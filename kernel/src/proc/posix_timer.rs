//! POSIX per-process timers: the kernel half of `timer_create(2)`,
//! `timer_settime(2)`, `timer_gettime(2)`, `timer_getoverrun(2)` and
//! `timer_delete(2)`, for the Linux ABI (syscalls 222-226) and for native
//! programs (`SYS_POSIX_TIMER`).
//!
//! A process may own any number of timers, each named by a small id that is
//! the process's own (0, 1, 2, ... in creation order, as Linux hands them
//! out). A timer measures `CLOCK_REALTIME`, `CLOCK_MONOTONIC`,
//! `CLOCK_BOOTTIME` or `CLOCK_TAI`, and when it expires either queues a signal
//! (`SIGEV_SIGNAL`, `SIGEV_THREAD`, `SIGEV_THREAD_ID`) or does nothing but
//! keep time (`SIGEV_NONE`).
//!
//! ## How an expiry becomes a signal
//!
//! An armed signal timer has one [`crate::hrtimer`] entry pending. Its
//! callback ([`fire`]) runs in the timer interrupt, and queues the timer's
//! signal through [`signal::post_timer_signal`] -- an entry of the timer's own
//! in the process's queue, Linux's preallocated per-timer `sigqueue`, so two
//! timers on one signal deliver twice and the signal layer can tell which
//! timer a dequeued signal came from.
//!
//! A **periodic** timer is not re-armed at expiry. It waits, "requeue
//! pending", until its signal is taken -- by delivery to a handler, by
//! `sigtimedwait`, by a `signalfd` read, or by being discarded -- and the
//! signal layer then calls [`signal_dequeued`], which moves the expiry past
//! now by whole intervals, counts the intervals it skipped as the **overrun**,
//! stamps that count into the delivered `siginfo` (`si_overrun`) and re-arms.
//! That is Linux's model (`posixtimer_rearm`), and it is what bounds the
//! cost of a short interval: one interrupt per delivered signal, however
//! long the signal sits blocked. `timer_getoverrun` answers the count of the
//! most recent delivery.
//!
//! An expiry that finds its signal ignored (and not blocked) queues nothing.
//! A periodic timer then re-arms itself at once -- at least a tick out, as
//! Linux 6.6 does -- counting what it skips, so the count is right when the
//! signal stops being ignored.
//!
//! ## Linux version
//!
//! Linux 6.6's behaviour, measured on the host (`build/ptimer_oracle.c`),
//! except where Linux 6.13 changed it for the better, which this follows:
//! a timer that is re-set or deleted while its signal is queued does not
//! deliver that signal (6.6 delivered it, stale), and re-setting a timer
//! resets its overrun count as well as the last-delivered one. POSIX leaves
//! both unspecified.
//!
//! ## Clocks
//!
//! `CLOCK_MONOTONIC` and `CLOCK_BOOTTIME` read [`hrtimer::now_ns`] (there is no
//! suspend, so they coincide), `CLOCK_REALTIME` and `CLOCK_TAI`
//! [`crate::timekeeping::clock_realtime`] (the TAI offset is 0). As on Linux, a
//! **relative** `CLOCK_REALTIME` timer is a monotonic one in disguise -- a
//! later `clock_settime` does not move it -- while an **absolute** one
//! (`TIMER_ABSTIME`) fires when the wall clock reads its time: a clock step
//! re-programs it ([`clock_was_set`]).
//!
//! The CPU-time clocks are not timeable here yet, since the kernel keeps no
//! per-task CPU time to fire on -- `timer_create` answers `EOPNOTSUPP` for
//! them (`known-issues/A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED.md`).
//! Neither are the alarm clocks, which need `CAP_WAKE_ALARM`.
//!
//! A timer's signal is only queued, from the interrupt, so a `SIGCONT` or a
//! fatal signal from a timer does not reach a *stopped* process until it is
//! continued (`known-issues/A-A-TIMER-SIGNAL-CANNOT-CONTINUE-OR-KILL-A-STOPPED-PROCESS.md`).
//!
//! ## Lifetime
//!
//! Timers are not inherited by `fork` and are deleted by `exec` ([`on_exec`])
//! and at exit ([`process_exit`]); the next id a process is given survives
//! `exec`, as Linux's does.
//!
//! ## Interrupt safety and lock order
//!
//! Timers live in a slab indexed straight from the callback's argument
//! (slot and generation), so the interrupt path never searches or allocates;
//! the slab grows only in `timer_create`. Lock order: [`TABLE`] →
//! the signal registries → the scheduler, and [`TABLE`] → the hrtimer's per-CPU
//! lock. The signal layer never calls in here holding its lock: it takes a
//! timer's entry out, lets go, and only then calls [`signal_dequeued`].

use crate::error::{KernelError, KernelResult};
use crate::hrtimer::{self, HrTimerHandle};
use crate::proc::pcb::ProcessId;
use crate::proc::signal::{self, SigInfo, TimerPost};
use crate::sched::task::TaskId;
use crate::serial_println;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// `CLOCK_REALTIME`.
pub const CLOCK_REALTIME: i32 = 0;
/// `CLOCK_MONOTONIC`.
pub const CLOCK_MONOTONIC: i32 = 1;
/// `CLOCK_BOOTTIME`.
pub const CLOCK_BOOTTIME: i32 = 7;
/// `CLOCK_TAI`.
pub const CLOCK_TAI: i32 = 11;

/// `SIGEV_SIGNAL`: queue a signal at expiry.
pub const SIGEV_SIGNAL: i32 = 0;
/// `SIGEV_NONE`: keep time, deliver nothing.
pub const SIGEV_NONE: i32 = 1;
/// `SIGEV_THREAD`: the C library's; to the kernel, a signal like
/// `SIGEV_SIGNAL` (Linux treats it so).
pub const SIGEV_THREAD: i32 = 2;
/// `SIGEV_THREAD_ID`: a signal to one thread (`SIGEV_SIGNAL | 4`).
pub const SIGEV_THREAD_ID: i32 = 4;

/// `TIMER_ABSTIME`: the value is a time on the clock, not a delay.
pub const TIMER_ABSTIME: u32 = 1;

/// The longest time a timer holds, in nanoseconds: Linux's `KTIME_MAX`
/// (`i64::MAX`), to which longer values saturate.
pub const KTIME_MAX: u64 = i64::MAX as u64;

/// The least delay with which a periodic timer whose signal is ignored
/// re-arms: one scheduler tick (10 ms at HZ=100), as Linux 6.6 holds an
/// ignored timer back by a jiffy so a tiny interval cannot spin the interrupt.
const IGNORED_REARM_MIN_NS: u64 = 10_000_000;

/// Largest timer id, as Linux allocates them (`INT_MAX`).
const MAX_TIMER_ID: i32 = i32::MAX;

/// `RLIMIT_SIGPENDING`, which a timer counts against as a queued signal does
/// (Linux preallocates each timer's `sigqueue` against it).
const RLIMIT_SIGPENDING: u32 = 11;

/// The clock a timer measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerClock {
    /// `CLOCK_REALTIME`.
    Realtime,
    /// `CLOCK_MONOTONIC`.
    Monotonic,
    /// `CLOCK_BOOTTIME`.
    Boottime,
    /// `CLOCK_TAI`.
    Tai,
}

impl TimerClock {
    /// The timeable clock a Linux clock id names, if it names one.
    #[must_use]
    pub const fn from_linux(id: i32) -> Option<Self> {
        match id {
            CLOCK_REALTIME => Some(Self::Realtime),
            CLOCK_MONOTONIC => Some(Self::Monotonic),
            CLOCK_BOOTTIME => Some(Self::Boottime),
            CLOCK_TAI => Some(Self::Tai),
            _ => None,
        }
    }

    /// Whether this is a wall clock, which `TIMER_ABSTIME` ties to the wall.
    const fn is_wall(self) -> bool {
        matches!(self, Self::Realtime | Self::Tai)
    }
}

/// What a timer does when it expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    /// `SIGEV_NONE`: nothing.
    None,
    /// Queue signal `signo` carrying `value` (`sigev_value`). `thread` is
    /// `SIGEV_THREAD_ID`'s target, recorded; the signal goes to the process
    /// as every signal does here until signals have per-thread state
    /// (`known-issues/A-SIGNALS-HAVE-NO-PER-THREAD-STATE.md`).
    Signal {
        /// The signal (1..=64).
        signo: u32,
        /// `sigev_value`, delivered as `si_value`.
        value: u64,
        /// `SIGEV_THREAD_ID`'s thread, if that was asked for.
        thread: Option<TaskId>,
    },
}

/// Why a timer operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerError {
    /// No timer of that id in the process (`EINVAL`).
    NoSuchTimer,
    /// Out of room: the `RLIMIT_SIGPENDING` charge, the ids, or memory
    /// (`EAGAIN`).
    Again,
}

/// Where a timer is in its cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Not armed (never, set to zero, or a one-shot that has fired).
    Disarmed,
    /// Armed: an `hrtimer` is pending (or, for `SIGEV_NONE`, only the expiry
    /// is kept).
    Armed,
    /// A periodic timer that fired and whose signal is queued: re-armed when
    /// the signal is taken ([`signal_dequeued`]).
    RequeuePending,
}

/// The clock an arming counts on: the wall for an absolute wall-clock arming,
/// the monotonic clock for everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    /// [`hrtimer::now_ns`].
    Monotonic,
    /// [`crate::timekeeping::clock_realtime`].
    Realtime,
}

impl Domain {
    /// The domain's current time.
    fn now(self) -> u64 {
        match self {
            Self::Monotonic => hrtimer::now_ns(),
            Self::Realtime => crate::timekeeping::clock_realtime(),
        }
    }
}

/// One timer.
#[derive(Debug, Clone, Copy)]
struct PosixTimer {
    /// Owning process.
    pid: ProcessId,
    /// Its id within the process.
    id: i32,
    /// `false` between [`reserve`] and [`commit`]: the id is taken, but the
    /// timer is not there yet for anything else to find.
    ready: bool,
    /// The clock it was created on.
    clock: TimerClock,
    /// What its expiry does.
    notify: Notify,
    /// Where it is in its cycle.
    status: Status,
    /// The clock the current arming counts on.
    domain: Domain,
    /// The next expiry, in `domain`'s time (meaningful while armed or
    /// requeue-pending).
    expiry: u64,
    /// The re-arm interval, 0 for a one-shot.
    interval: u64,
    /// Linux's `it_overrun`: the intervals skipped since the last delivery,
    /// minus one; -1 right after an arming or a delivery.
    overrun: i64,
    /// What `timer_getoverrun` answers: the overrun of the last delivery.
    overrun_last: i64,
    /// The arming's generation: set anew by every `timer_settime`, and carried
    /// by each signal the arming queues, so a signal of an earlier arming is
    /// known stale when it is taken.
    arm_gen: u64,
    /// The pending `hrtimer`, if any.
    hr: Option<HrTimerHandle>,
}

impl PosixTimer {
    /// Whether the timer delivers nothing.
    fn sig_none(&self) -> bool {
        self.notify == Notify::None
    }
}

/// A slab slot. `hr_gen` outlives the timer in it, so a callback minted for
/// an earlier occupant can never match a later one.
#[derive(Debug)]
struct Slot {
    /// Bumped at every `hrtimer` arming and cancel; the callback's argument
    /// carries the value it was armed with.
    hr_gen: u32,
    /// The occupant.
    timer: Option<PosixTimer>,
}

/// A process's view of its timers.
#[derive(Debug, Default)]
struct ProcTimers {
    /// Where the next id search starts (Linux's `next_posix_timer_id`).
    next_id: i32,
    /// Its timers, by id, to their slots.
    ids: BTreeMap<i32, u32>,
}

/// Every timer.
struct Table {
    /// Timers by slot.
    slots: Vec<Slot>,
    /// Empty slots, to reuse.
    free: Vec<u32>,
    /// Per-process ids and id allocation.
    procs: BTreeMap<ProcessId, ProcTimers>,
    /// Source of [`PosixTimer::arm_gen`] values.
    next_gen: u64,
}

impl Table {
    /// An empty table.
    const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            procs: BTreeMap::new(),
            next_gen: 1,
        }
    }

    /// A fresh arming generation.
    fn fresh_gen(&mut self) -> u64 {
        let g = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1).max(1);
        g
    }

    /// The slot of `pid`'s timer `id`, ready or not.
    fn slot_of(&self, pid: ProcessId, id: i32) -> Option<u32> {
        self.procs.get(&pid)?.ids.get(&id).copied()
    }

    /// The slot index and the timer in it, for a **ready** timer.
    fn ready_mut(&mut self, pid: ProcessId, id: i32) -> Option<(usize, &mut Slot)> {
        let idx = self.slot_of(pid, id)? as usize;
        let slot = self.slots.get_mut(idx)?;
        if slot.timer.as_ref().is_some_and(|t| t.ready && t.pid == pid) {
            Some((idx, slot))
        } else {
            None
        }
    }

    /// Put `timer` in a slot, reusing an empty one if there is one.
    fn alloc_slot(&mut self, timer: PosixTimer) -> Result<u32, TimerError> {
        if let Some(idx) = self.free.pop() {
            if let Some(slot) = self.slots.get_mut(idx as usize) {
                slot.timer = Some(timer);
                return Ok(idx);
            }
        }
        let idx = u32::try_from(self.slots.len()).map_err(|_| TimerError::Again)?;
        self.slots.try_reserve(1).map_err(|_| TimerError::Again)?;
        self.slots.push(Slot {
            hr_gen: 0,
            timer: Some(timer),
        });
        Ok(idx)
    }

    /// Empty slot `idx` -- cancelling its `hrtimer` and taking its queued
    /// signal off the queue -- and forget the timer's id. The next id a
    /// process is given is untouched.
    fn free_timer(&mut self, idx: u32) {
        let Some(slot) = self.slots.get_mut(idx as usize) else {
            return;
        };
        let Some(timer) = slot.timer.take() else {
            return;
        };
        if let Some(h) = timer.hr {
            // `false` means it fired already or is firing on another CPU: the
            // generation bump below turns that callback into a no-op.
            hrtimer::cancel(h);
        }
        slot.hr_gen = slot.hr_gen.wrapping_add(1);
        if timer.ready {
            // Linux 6.13: a deleted timer's queued signal is not delivered.
            signal::remove_timer_signal(timer.pid, timer.id);
        }
        if let Some(p) = self.procs.get_mut(&timer.pid) {
            p.ids.remove(&timer.id);
        }
        // A slot that cannot go on the free list is simply not reused.
        if self.free.try_reserve(1).is_ok() {
            self.free.push(idx);
        }
    }
}

/// All timers. Taken with interrupts off ([`with_table`]): the expiry
/// callback takes it in the timer interrupt.
static TABLE: Mutex<Table> = Mutex::new(Table::new());

/// Run `f` with [`TABLE`] held and interrupts disabled.
fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    crate::cpu::without_interrupts(|| f(&mut TABLE.lock()))
}

/// The `hrtimer` argument for slot `idx` armed at generation `generation`.
const fn hr_arg(idx: usize, generation: u32) -> u64 {
    ((idx as u64) << 32) | generation as u64
}

/// Arm `timer` (in slot `idx`, whose generation is `hr_gen`) to fire at its
/// expiry, `now` being the current time in its domain.
fn arm_hr(idx: usize, hr_gen: &mut u32, timer: &mut PosixTimer, now: u64) {
    if let Some(h) = timer.hr.take() {
        hrtimer::cancel(h);
    }
    *hr_gen = hr_gen.wrapping_add(1);
    let delay = timer.expiry.saturating_sub(now);
    timer.hr = Some(hrtimer::schedule_ns(delay, fire, hr_arg(idx, *hr_gen)));
    timer.status = Status::Armed;
}

/// Linux's `hrtimer_forward`: if `expiry` is not after `now`, move it past
/// `now` by whole intervals and answer how many; else answer 0. An interval of
/// 0 never moves.
#[must_use]
fn forward(expiry: &mut u64, interval: u64, now: u64) -> u64 {
    if interval == 0 || now < *expiry {
        return 0;
    }
    // `now >= *expiry` here, so the subtraction cannot wrap; checked_div by a
    // non-zero interval cannot fail.
    let behind = now.saturating_sub(*expiry);
    let skipped = behind.checked_div(interval).unwrap_or(0).saturating_add(1);
    *expiry = expiry
        .saturating_add(skipped.saturating_mul(interval))
        .min(KTIME_MAX);
    skipped
}

/// Linux's `timer_overrun_to_int`: an overrun count as `si_overrun` and
/// `timer_getoverrun` report it, held to `0..=DELAYTIMER_MAX` (`INT_MAX`).
#[must_use]
fn overrun_to_int(overrun: i64) -> i32 {
    i32::try_from(overrun.max(0)).unwrap_or(i32::MAX)
}

/// Add `skipped` intervals to an overrun count, saturating.
fn add_overrun(overrun: &mut i64, skipped: u64) {
    *overrun = overrun.saturating_add(i64::try_from(skipped).unwrap_or(i64::MAX));
}

/// The `hrtimer` callback: timer slot `arg >> 32` has expired, if its
/// generation is still `arg as u32`. Runs in the timer interrupt.
fn fire(arg: u64) {
    #[allow(clippy::cast_possible_truncation)]
    let (idx, generation) = ((arg >> 32) as usize, arg as u32);
    with_table(|t| {
        let Some(slot) = t.slots.get_mut(idx) else {
            return;
        };
        let Slot { hr_gen, timer } = slot;
        if *hr_gen != generation {
            return; // re-armed, cancelled or freed since
        }
        let Some(timer) = timer.as_mut() else {
            return;
        };
        if !timer.ready || timer.status != Status::Armed || timer.hr.is_none() {
            return;
        }
        timer.hr = None;
        let now = timer.domain.now();
        if now < timer.expiry {
            // Early: the wall clock was stepped back after the arming (a
            // monotonic arming is never early). Wait out the rest.
            arm_hr(idx, hr_gen, timer, now);
            return;
        }
        let Notify::Signal { signo, value, .. } = timer.notify else {
            // SIGEV_NONE arms no hrtimer.
            return;
        };
        let periodic = timer.interval > 0;
        timer.status = if periodic {
            Status::RequeuePending
        } else {
            Status::Disarmed
        };
        let info = SigInfo::timer(timer.id, value);
        match signal::post_timer_signal(timer.pid, signo, timer.id, timer.arm_gen, info) {
            TimerPost::Queued | TimerPost::AlreadyQueued => {}
            TimerPost::Ignored => {
                if periodic {
                    // Nothing was queued, so nothing will be taken to re-arm
                    // it: re-arm now, at least a tick out, counting the
                    // intervals skipped (Linux 6.6's `posix_timer_fn`).
                    let base = if timer.interval < IGNORED_REARM_MIN_NS {
                        now.saturating_add(IGNORED_REARM_MIN_NS)
                    } else {
                        now
                    };
                    let skipped = forward(&mut timer.expiry, timer.interval, base);
                    add_overrun(&mut timer.overrun, skipped);
                    arm_hr(idx, hr_gen, timer, now);
                }
            }
            TimerPost::Gone => timer.status = Status::Disarmed,
        }
    });
}

/// The signal layer has taken `pid`'s timer `id`'s queued signal -- to
/// deliver it, or to discard it -- with record `info`, queued by the arming
/// `token` names. Answers the record to deliver, or `None` if the signal is
/// stale (the timer re-set or deleted since it was queued), in which case it
/// is not delivered.
///
/// For a periodic timer waiting on this signal it is Linux's
/// `posixtimer_rearm`: the expiry moves past now by whole intervals, those
/// intervals count as the overrun -- the delivered `si_overrun`, and what
/// `timer_getoverrun` answers from now on -- and the timer re-arms.
///
/// Must not be called with the signal lock held.
#[must_use]
pub fn signal_dequeued(pid: ProcessId, id: i32, token: u64, mut info: SigInfo) -> Option<SigInfo> {
    with_table(|t| {
        let (idx, slot) = t.ready_mut(pid, id)?;
        let Slot { hr_gen, timer } = slot;
        let timer = timer.as_mut()?;
        if timer.arm_gen != token {
            return None;
        }
        if timer.status == Status::RequeuePending && timer.interval > 0 {
            let now = timer.domain.now();
            let skipped = forward(&mut timer.expiry, timer.interval, now);
            add_overrun(&mut timer.overrun, skipped);
            arm_hr(idx, hr_gen, timer, now);
            timer.overrun_last = timer.overrun;
            timer.overrun = -1;
            // si_overrun shares si_uid's slot (SI_TIMER's record layout).
            #[allow(clippy::cast_sign_loss)]
            {
                info.sender_uid = overrun_to_int(timer.overrun_last) as u32;
            }
        }
        Some(info)
    })
}

/// Charge one more timer to the user of `pid` against its
/// `RLIMIT_SIGPENDING` -- the queued signals and timers of all its processes
/// -- answering whether it fits. Linux counts a timer's preallocated
/// `sigqueue` there. A process with no user (the kernel's) is not limited.
fn charge_fits(pid: ProcessId) -> bool {
    use crate::proc::pcb;
    let Some(uid) = pcb::process_uid(pid) else {
        return true;
    };
    let limit = pcb::get_rlimit(pid, RLIMIT_SIGPENDING).map_or(u64::MAX, |(soft, _)| soft);
    let pids = pcb::pids_of_user(uid);
    let queued: u64 = pids
        .iter()
        .map(|&p| u64::from(signal::sets(p).pending.count_ones()))
        .fold(0, u64::saturating_add);
    let timers = with_table(|t| {
        pids.iter()
            .filter_map(|p| t.procs.get(p))
            .map(|p| p.ids.len() as u64)
            .fold(0, u64::saturating_add)
    });
    queued.saturating_add(timers) < limit
}

/// Take an id for a new timer of `pid` -- the first free one from where the
/// last search stopped, as Linux allocates them -- holding it for the timer
/// [`commit`] will put there. Until then nothing else finds the timer.
///
/// # Errors
///
/// [`TimerError::Again`] if the user's `RLIMIT_SIGPENDING` has no room for
/// it, if every id is taken, or if memory runs out.
pub fn reserve(pid: ProcessId) -> Result<i32, TimerError> {
    if !charge_fits(pid) {
        return Err(TimerError::Again);
    }
    with_table(|t| {
        let proc_timers = t.procs.entry(pid).or_default();
        let mut found = None;
        // Every id once at most; the first free one ends it, and the process
        // holds no more timers than its RLIMIT_SIGPENDING lets it.
        for _ in 0..=MAX_TIMER_ID {
            let id = proc_timers.next_id;
            // Clamped to the positive space, as Linux's `(id + 1) & INT_MAX`.
            proc_timers.next_id = id.checked_add(1).unwrap_or(0);
            if !proc_timers.ids.contains_key(&id) {
                found = Some(id);
                break;
            }
        }
        let id = found.ok_or(TimerError::Again)?;
        let placeholder = PosixTimer {
            pid,
            id,
            ready: false,
            clock: TimerClock::Monotonic,
            notify: Notify::None,
            status: Status::Disarmed,
            domain: Domain::Monotonic,
            expiry: 0,
            interval: 0,
            overrun: -1,
            overrun_last: 0,
            arm_gen: 0,
            hr: None,
        };
        let idx = t.alloc_slot(placeholder)?;
        if let Some(p) = t.procs.get_mut(&pid) {
            p.ids.insert(id, idx);
        }
        Ok(id)
    })
}

/// Make the timer whose id [`reserve`] took real: disarmed, on `clock`,
/// doing `notify` at expiry. Reserves its signal's room in the process's
/// queue first, so its expiry never allocates.
///
/// # Errors
///
/// [`TimerError::NoSuchTimer`] if `id` was not reserved; [`TimerError::Again`]
/// if the room cannot be had (the id is then released).
pub fn commit(
    pid: ProcessId,
    id: i32,
    clock: TimerClock,
    notify: Notify,
) -> Result<(), TimerError> {
    let result = with_table(|t| {
        let idx = t.slot_of(pid, id).ok_or(TimerError::NoSuchTimer)?;
        let count = t.procs.get(&pid).map_or(0, |p| p.ids.len());
        // TABLE -> signal lock is the order everything here takes them in.
        signal::reserve_timer_signals(pid, count).map_err(|_| TimerError::Again)?;
        let generation = t.fresh_gen();
        let timer = t
            .slots
            .get_mut(idx as usize)
            .and_then(|s| s.timer.as_mut())
            .filter(|tm| tm.pid == pid && !tm.ready)
            .ok_or(TimerError::NoSuchTimer)?;
        timer.clock = clock;
        timer.notify = notify;
        timer.arm_gen = generation;
        timer.ready = true;
        Ok(())
    });
    if result == Err(TimerError::Again) {
        release(pid, id);
    }
    result
}

/// Give back an id [`reserve`] took for a timer that is not going to be
/// created. The next id stays where the reservation moved it, as on Linux.
pub fn release(pid: ProcessId, id: i32) {
    with_table(|t| {
        if let Some(idx) = t.slot_of(pid, id) {
            let unready = t
                .slots
                .get(idx as usize)
                .and_then(|s| s.timer.as_ref())
                .is_some_and(|tm| !tm.ready);
            if unready {
                t.free_timer(idx);
            }
        }
    });
}

/// `timer_gettime`'s answer for `timer`: the time to its next expiry and its
/// interval, Linux's `common_timer_get` -- which, for a periodic timer waiting
/// on its signal or one that delivers nothing, first moves the expiry past
/// now, counting the intervals as overrun.
fn current(timer: &mut PosixTimer) -> (u64, u64) {
    let iv = timer.interval;
    if timer.status == Status::Disarmed {
        return (0, iv);
    }
    let now = timer.domain.now();
    if iv > 0 && (timer.status == Status::RequeuePending || timer.sig_none()) {
        let skipped = forward(&mut timer.expiry, iv, now);
        add_overrun(&mut timer.overrun, skipped);
    }
    let value = if timer.expiry > now {
        timer.expiry.saturating_sub(now)
    } else if timer.sig_none() {
        // An expired one-shot that delivers nothing reads 0...
        0
    } else {
        // ...but one whose signal is on its way reads 1 ns, not 0.
        1
    };
    (value, iv)
}

/// `timer_settime`: arm `pid`'s timer `id` to expire after `value` ns -- or
/// at `value` on its clock, for `abstime` -- and every `interval` ns after,
/// or disarm it if `value` is 0. Answers the setting it had, as
/// `timer_gettime` would have.
///
/// A signal of the timer's still queued is taken off the queue (Linux 6.13
/// drops it), and both overrun counts start again.
///
/// # Errors
///
/// [`TimerError::NoSuchTimer`] if the process has no such timer.
pub fn settime(
    pid: ProcessId,
    id: i32,
    abstime: bool,
    value: u64,
    interval: u64,
) -> Result<(u64, u64), TimerError> {
    with_table(|t| {
        let generation = t.fresh_gen();
        let (idx, slot) = t.ready_mut(pid, id).ok_or(TimerError::NoSuchTimer)?;
        let Slot { hr_gen, timer } = slot;
        let timer = timer.as_mut().ok_or(TimerError::NoSuchTimer)?;
        let old = current(timer);
        if let Some(h) = timer.hr.take() {
            hrtimer::cancel(h);
        }
        *hr_gen = hr_gen.wrapping_add(1);
        signal::remove_timer_signal(pid, id);
        timer.arm_gen = generation;
        timer.status = Status::Disarmed;
        timer.overrun = -1;
        timer.overrun_last = 0;
        timer.interval = 0;
        if value == 0 {
            return Ok(old);
        }
        timer.interval = interval.min(KTIME_MAX);
        timer.domain = if abstime && timer.clock.is_wall() {
            Domain::Realtime
        } else {
            Domain::Monotonic
        };
        let now = timer.domain.now();
        timer.expiry = if abstime {
            value.min(KTIME_MAX)
        } else {
            now.saturating_add(value).min(KTIME_MAX)
        };
        if timer.sig_none() {
            // Nothing to fire: the expiry is only for timer_gettime.
            timer.status = Status::Armed;
        } else {
            arm_hr(idx, hr_gen, timer, now);
        }
        Ok(old)
    })
}

/// `timer_gettime`: `pid`'s timer `id`'s time to its next expiry and its
/// interval, in nanoseconds.
///
/// # Errors
///
/// [`TimerError::NoSuchTimer`] if the process has no such timer.
pub fn gettime(pid: ProcessId, id: i32) -> Result<(u64, u64), TimerError> {
    with_table(|t| {
        let (_, slot) = t.ready_mut(pid, id).ok_or(TimerError::NoSuchTimer)?;
        let timer = slot.timer.as_mut().ok_or(TimerError::NoSuchTimer)?;
        Ok(current(timer))
    })
}

/// `timer_getoverrun`: the overrun count of the last signal `pid`'s timer
/// `id` delivered (0 if it has delivered none since it was set).
///
/// # Errors
///
/// [`TimerError::NoSuchTimer`] if the process has no such timer.
pub fn getoverrun(pid: ProcessId, id: i32) -> Result<i32, TimerError> {
    with_table(|t| {
        let (_, slot) = t.ready_mut(pid, id).ok_or(TimerError::NoSuchTimer)?;
        let timer = slot.timer.as_ref().ok_or(TimerError::NoSuchTimer)?;
        Ok(overrun_to_int(timer.overrun_last))
    })
}

/// `timer_delete`: delete `pid`'s timer `id`, and its queued signal if it has
/// one (Linux 6.13 does not deliver it).
///
/// # Errors
///
/// [`TimerError::NoSuchTimer`] if the process has no such timer.
pub fn delete(pid: ProcessId, id: i32) -> Result<(), TimerError> {
    with_table(|t| {
        let (idx, _) = t.ready_mut(pid, id).ok_or(TimerError::NoSuchTimer)?;
        // Slab indices come from u32s.
        #[allow(clippy::cast_possible_truncation)]
        t.free_timer(idx as u32);
        Ok(())
    })
}

/// Delete every timer `pid` has, reserved ones included.
fn delete_all(t: &mut Table, pid: ProcessId) {
    let slots: Vec<u32> = t
        .procs
        .get(&pid)
        .map(|p| p.ids.values().copied().collect())
        .unwrap_or_default();
    for idx in slots {
        t.free_timer(idx);
    }
}

/// `exec`: delete the process's timers (POSIX), keeping where its next id
/// search starts, as Linux keeps `next_posix_timer_id`.
pub fn on_exec(pid: ProcessId) {
    with_table(|t| delete_all(t, pid));
}

/// Process teardown: delete its timers and forget it.
pub fn process_exit(pid: ProcessId) {
    with_table(|t| {
        delete_all(t, pid);
        t.procs.remove(&pid);
    });
}

/// How many timers `pid` has.
#[must_use]
pub fn count(pid: ProcessId) -> usize {
    with_table(|t| t.procs.get(&pid).map_or(0, |p| p.ids.len()))
}

/// The wall clock was stepped (`clock_settime`, `settimeofday`,
/// `ADJ_SETOFFSET`): re-program every armed timer that counts on it, so it
/// fires when the wall clock reads its time -- Linux's `clock_was_set`.
pub fn clock_was_set() {
    with_table(|t| {
        let now = Domain::Realtime.now();
        for (idx, slot) in t.slots.iter_mut().enumerate() {
            let Slot { hr_gen, timer } = slot;
            let Some(timer) = timer.as_mut() else {
                continue;
            };
            if timer.ready
                && timer.domain == Domain::Realtime
                && timer.status == Status::Armed
                && timer.hr.is_some()
            {
                arm_hr(idx, hr_gen, timer, now);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Synthetic PID base for self-tests (well outside any real PID range).
const TEST_PID_BASE: ProcessId = 0xFFFF_5180_0000;

/// Log and fail unless `cond`.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[posix_timer]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

/// Create a timer the way the syscalls do: reserve, then commit.
fn create(pid: ProcessId, clock: TimerClock, notify: Notify) -> Result<i32, TimerError> {
    let id = reserve(pid)?;
    commit(pid, id, clock, notify)?;
    Ok(id)
}

/// Expire `pid`'s timer `id` now, deterministically: cancel its real
/// `hrtimer` and run the callback by hand, as if it had fired. Answers
/// whether there was an armed timer to expire.
fn expire_for_test(pid: ProcessId, id: i32) -> bool {
    let arg = with_table(|t| {
        let (idx, slot) = t.ready_mut(pid, id)?;
        let timer = slot.timer.as_mut()?;
        // Due now, so the callback does not take it for an early one.
        timer.expiry = timer.domain.now();
        let handle = timer.hr?;
        // Keep `hr` set: the callback requires it. Cancel the real entry so
        // it cannot fire later (it would be stale anyway).
        hrtimer::cancel(handle);
        Some(hr_arg(idx, slot.hr_gen))
    });
    match arg {
        Some(a) => {
            fire(a);
            true
        }
        None => false,
    }
}

/// Move `pid`'s timer `id`'s expiry `by` nanoseconds into the past.
fn rewind_for_test(pid: ProcessId, id: i32, by: u64) {
    with_table(|t| {
        if let Some((_, slot)) = t.ready_mut(pid, id) {
            if let Some(timer) = slot.timer.as_mut() {
                timer.expiry = timer.domain.now().saturating_sub(by);
            }
        }
    });
}

/// Whether `pid`'s timer `id` has an `hrtimer` pending.
fn armed_for_test(pid: ProcessId, id: i32) -> bool {
    with_table(|t| {
        t.ready_mut(pid, id)
            .and_then(|(_, s)| s.timer.as_ref())
            .is_some_and(|tm| tm.status == Status::Armed && tm.hr.is_some())
    })
}

/// A signal timer notify.
const fn sig(signo: u32, value: u64) -> Notify {
    Notify::Signal {
        signo,
        value,
        thread: None,
    }
}

/// The record `take_pending_info_in_mask` should hand back for timer `id`.
fn is_timer_record(info: &SigInfo, id: i32, value: u64) -> bool {
    #[allow(clippy::cast_sign_loss)]
    let tid = id as u32;
    info.code == signal::si_code::SI_TIMER && info.sender_pid == tid && info.value == value
}

/// POSIX timer self-tests: the pure overrun arithmetic, id allocation, the
/// expiry path (driven by hand, so it does not depend on interrupt timing),
/// the signal queue's ordering and coalescing, re-arming on dequeue with the
/// overrun count, ignored and blocked signals, re-setting and deleting a
/// timer whose signal is queued, `SIGEV_NONE`, and exec and exit teardown.
///
/// Every timer a test leaves armed is at least 50 s from expiring, so no real
/// interrupt can race the hand-driven expiries, however long the emulator
/// stalls (`known-issues/A-TIMING-SELF-TESTS-READ-A-HOST-STALL-AS-A-KERNEL-BUG.md`).
/// The overrun test therefore counts on the wall clock, the one clock far
/// enough from zero to wind a timer thousands of seconds into the past.
pub fn self_test() -> KernelResult<()> {
    const SEC: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;
    const SIGUSR1: u32 = 10;
    const SIGUSR2: u32 = 12;
    const SIGRT: u32 = 36;
    serial_println!("[posix_timer] Running POSIX timer self-test...");
    check(
        crate::timekeeping::is_initialized(),
        "the wall clock runs (timekeeping::init precedes this test)",
    )?;

    // --- forward (Linux's hrtimer_forward) ---
    let mut e = 100;
    check(
        forward(&mut e, 10, 99) == 0 && e == 100,
        "forward: not yet due",
    )?;
    check(
        forward(&mut e, 10, 100) == 1 && e == 110,
        "forward: due exactly",
    )?;
    let mut e = 100;
    check(
        forward(&mut e, 10, 155) == 6 && e == 160,
        "forward: 5.5 intervals late",
    )?;
    let mut e = 100;
    check(
        forward(&mut e, 0, 1000) == 0 && e == 100,
        "forward: one-shot",
    )?;
    check(overrun_to_int(-1) == 0, "overrun -1 reads 0")?;
    check(
        overrun_to_int(i64::MAX) == i32::MAX,
        "overrun clamps to INT_MAX",
    )?;
    serial_println!("[posix_timer]   forward / overrun arithmetic: OK");

    // --- ids: sequential, not lowest-free; a failed create still uses one ---
    let p = TEST_PID_BASE + 1;
    let ids = (
        create(p, TimerClock::Monotonic, sig(SIGUSR1, 0)),
        create(p, TimerClock::Monotonic, sig(SIGUSR1, 0)),
        create(p, TimerClock::Monotonic, sig(SIGUSR1, 0)),
    );
    check(ids == (Ok(0), Ok(1), Ok(2)), "ids 0, 1, 2")?;
    check(delete(p, 1).is_ok(), "delete 1")?;
    check(
        delete(p, 1) == Err(TimerError::NoSuchTimer),
        "delete 1 twice",
    )?;
    check(
        gettime(p, 1) == Err(TimerError::NoSuchTimer),
        "gettime deleted",
    )?;
    check(
        create(p, TimerClock::Monotonic, sig(SIGUSR1, 0)) == Ok(3),
        "next id 3, not the freed 1",
    )?;
    check(reserve(p) == Ok(4), "reserve 4")?;
    check(
        gettime(p, 4) == Err(TimerError::NoSuchTimer),
        "a reserved id is not a timer",
    )?;
    release(p, 4);
    check(
        create(p, TimerClock::Monotonic, sig(SIGUSR1, 0)) == Ok(5),
        "a released id is still used up",
    )?;
    check(count(p) == 4, "four timers")?;
    check(
        signal::timer_signals(p).1 >= 4,
        "room reserved for four signals",
    )?;
    process_exit(p);
    check(count(p) == 0, "exit deletes them")?;
    signal::remove(p);
    serial_println!("[posix_timer]   id allocation: OK");

    // --- one-shot: arm, read back, expire by hand ---
    let p = TEST_PID_BASE + 2;
    let id = create(
        p,
        TimerClock::Monotonic,
        sig(SIGUSR1, 0x1122_3344_5566_7788),
    )
    .map_err(|_| KernelError::InternalError)?;
    check(
        settime(p, id, false, 1000 * SEC, 0) == Ok((0, 0)),
        "first arming: old 0",
    )?;
    let (v, iv) = gettime(p, id).map_err(|_| KernelError::InternalError)?;
    check(
        iv == 0 && v > 999 * SEC && v <= 1000 * SEC,
        "one-shot reads ~1000s",
    )?;
    check(getoverrun(p, id) == Ok(0), "no overrun yet")?;
    check(expire_for_test(p, id), "one-shot expires")?;
    check(
        signal::pending(p) & (1 << (SIGUSR1 - 1)) != 0,
        "SIGUSR1 pending",
    )?;
    check(gettime(p, id) == Ok((0, 0)), "fired one-shot reads 0")?;
    let got = signal::take_pending_info_in_mask(p, 1 << (SIGUSR1 - 1));
    check(
        got.is_some_and(|(s, info)| {
            s == SIGUSR1
                && is_timer_record(&info, id, 0x1122_3344_5566_7788)
                && info.sender_uid == 0
        }),
        "SI_TIMER record: id, value, overrun 0",
    )?;
    check(signal::pending(p) == 0, "nothing left pending")?;
    check(!armed_for_test(p, id), "a one-shot stays disarmed")?;
    serial_println!("[posix_timer]   one-shot expiry and record: OK");

    // --- periodic: re-armed at dequeue, overrun counted ---
    let w =
        create(p, TimerClock::Realtime, sig(SIGUSR1, 5)).map_err(|_| KernelError::InternalError)?;
    let wall = Domain::Realtime.now();
    check(
        settime(p, w, true, wall.saturating_add(1000 * SEC), 100 * SEC).is_ok(),
        "arm periodic on the wall clock",
    )?;
    check(expire_for_test(p, w), "periodic expires")?;
    check(
        !armed_for_test(p, w),
        "requeue pending: not re-armed at expiry",
    )?;
    // Make it 5550 s late: 55 intervals skipped beyond the one that fired.
    rewind_for_test(p, w, 5550 * SEC);
    let got = signal::take_pending_info_in_mask(p, 1 << (SIGUSR1 - 1));
    let overrun = got.map(|(_, info)| info.sender_uid);
    // 55 exactly, plus one for every 100 s the test itself took.
    check(
        overrun.is_some_and(|o| (55..=60).contains(&o)),
        "si_overrun counts the skipped intervals",
    )?;
    #[allow(clippy::cast_possible_wrap)]
    let overrun_i = overrun.map(|o| o as i32);
    check(
        getoverrun(p, w).ok() == overrun_i,
        "timer_getoverrun agrees",
    )?;
    check(armed_for_test(p, w), "re-armed at dequeue")?;
    let (v, iv) = gettime(p, w).map_err(|_| KernelError::InternalError)?;
    check(
        iv == 100 * SEC && v > 0 && v <= 100 * SEC,
        "periodic reads within an interval",
    )?;
    let old = settime(p, w, false, 0, 0).map_err(|_| KernelError::InternalError)?;
    check(old.1 == 100 * SEC, "disarm reports the old interval")?;
    check(gettime(p, w) == Ok((0, 0)), "disarmed reads 0, interval 0")?;
    check(getoverrun(p, w) == Ok(0), "settime resets the overrun")?;
    check(signal::pending(p) == 0, "nothing pending")?;
    serial_println!("[posix_timer]   periodic re-arm on dequeue + overrun: OK");

    // --- two timers on one signal deliver twice; kill coalesces behind them ---
    let a = create(p, TimerClock::Monotonic, sig(SIGUSR2, 111))
        .map_err(|_| KernelError::InternalError)?;
    let b = create(p, TimerClock::Monotonic, sig(SIGUSR2, 222))
        .map_err(|_| KernelError::InternalError)?;
    check(settime(p, a, false, 1000 * SEC, 0).is_ok(), "arm a")?;
    check(settime(p, b, false, 1000 * SEC, 0).is_ok(), "arm b")?;
    check(
        expire_for_test(p, a) && expire_for_test(p, b),
        "a and b expire",
    )?;
    check(
        !signal::set_pending_info(p, SIGUSR2, SigInfo::user(1, 0)),
        "a kill of a signal a timer has queued is merged",
    )?;
    let mask2 = 1u64 << (SIGUSR2 - 1);
    let first = signal::take_pending_info_in_mask(p, mask2);
    let second = signal::take_pending_info_in_mask(p, mask2);
    let third = signal::take_pending_info_in_mask(p, mask2);
    check(
        first.is_some_and(|(_, i)| is_timer_record(&i, a, 111))
            && second.is_some_and(|(_, i)| is_timer_record(&i, b, 222))
            && third.is_none(),
        "both timers' signals, in order, and nothing else",
    )?;
    // A kill first: its record is delivered before the timer's.
    check(
        signal::set_pending_info(p, SIGUSR2, SigInfo::user(1, 0)),
        "kill first",
    )?;
    check(settime(p, a, false, 1000 * SEC, 0).is_ok(), "re-arm a")?;
    check(expire_for_test(p, a), "a expires again")?;
    let first = signal::take_pending_info_in_mask(p, mask2);
    let second = signal::take_pending_info_in_mask(p, mask2);
    check(
        first.is_some_and(|(_, i)| i.code == signal::si_code::SI_USER)
            && second.is_some_and(|(_, i)| is_timer_record(&i, a, 111)),
        "the kill, then the timer",
    )?;
    serial_println!("[posix_timer]   per-timer queue entries and ordering: OK");

    // --- re-setting or deleting a timer drops its queued signal ---
    check(settime(p, a, false, 1000 * SEC, 0).is_ok(), "arm a")?;
    check(expire_for_test(p, a), "a expires")?;
    check(signal::pending(p) & mask2 != 0, "queued")?;
    check(settime(p, a, false, 1000 * SEC, 0).is_ok(), "re-set a")?;
    check(
        signal::pending(p) & mask2 == 0,
        "re-setting drops the queued signal",
    )?;
    check(
        settime(p, b, false, 1000 * SEC, 0).is_ok() && expire_for_test(p, b),
        "b expires",
    )?;
    check(delete(p, b).is_ok(), "delete b")?;
    check(
        signal::pending(p) & mask2 == 0,
        "deleting drops the queued signal",
    )?;
    // An entry taken before its timer was re-set is refused by its token.
    check(
        signal_dequeued(p, a, 0, SigInfo::timer(a, 111)).is_none(),
        "a stale entry is not delivered",
    )?;
    serial_println!("[posix_timer]   re-set / delete drop the queued signal: OK");

    // --- ignored: nothing queued; a periodic timer re-arms itself ---
    let bit1 = 1u64 << (SIGUSR1 - 1);
    check(
        signal::set_ignored(p, bit1, false).is_ok(),
        "ignore SIGUSR1",
    )?;
    check(
        settime(p, id, false, 1000 * SEC, 100 * SEC).is_ok(),
        "arm periodic",
    )?;
    check(expire_for_test(p, id), "expires while ignored")?;
    check(
        signal::pending(p) & bit1 == 0,
        "an ignored signal is not queued",
    )?;
    check(armed_for_test(p, id), "the periodic timer re-armed itself")?;
    check(settime(p, id, false, 1000 * SEC, 0).is_ok(), "arm one-shot")?;
    check(expire_for_test(p, id), "one-shot expires while ignored")?;
    check(
        !armed_for_test(p, id) && gettime(p, id) == Ok((0, 0)),
        "and is done",
    )?;
    // Blocked and ignored: queued (the action may change before unblocking),
    // then discarded at delivery -- re-arming the periodic timer.
    signal::set_blocked(p, bit1);
    check(
        settime(p, id, false, 1000 * SEC, 100 * SEC).is_ok(),
        "arm periodic",
    )?;
    check(expire_for_test(p, id), "expires blocked")?;
    check(
        signal::pending(p) & bit1 != 0,
        "blocked: queued though ignored",
    )?;
    signal::set_blocked(p, 0);
    check(
        signal::take_deliverable_info(p).is_none(),
        "unblocked and ignored: discarded",
    )?;
    check(signal::pending(p) & bit1 == 0, "nothing pending")?;
    check(armed_for_test(p, id), "the discard re-armed the timer")?;
    check(signal::set_ignored(p, 0, false).is_ok(), "stop ignoring")?;
    // Becoming ignored while queued discards it, and re-arms too.
    signal::set_blocked(p, bit1);
    check(expire_for_test(p, id), "expires blocked again")?;
    check(signal::pending(p) & bit1 != 0, "queued")?;
    signal::record_disposition(p, SIGUSR1, signal::Disposition::Ignore, false);
    check(signal::pending(p) & bit1 == 0, "SIG_IGN discards it")?;
    check(armed_for_test(p, id), "and the timer re-armed")?;
    signal::record_disposition(p, SIGUSR1, signal::Disposition::Default, false);
    signal::set_blocked(p, 0);
    serial_println!("[posix_timer]   ignored and blocked signals: OK");

    // --- SIGEV_NONE: keeps time, delivers nothing ---
    let n =
        create(p, TimerClock::Monotonic, Notify::None).map_err(|_| KernelError::InternalError)?;
    check(
        settime(p, n, false, 1000 * SEC, 0).is_ok(),
        "arm SIGEV_NONE",
    )?;
    check(
        with_table(|t| {
            t.ready_mut(p, n)
                .and_then(|(_, s)| s.timer)
                .is_some_and(|tm| tm.hr.is_none())
        }),
        "no hrtimer for SIGEV_NONE",
    )?;
    let (v, _) = gettime(p, n).map_err(|_| KernelError::InternalError)?;
    check(v > 999 * SEC, "SIGEV_NONE reads its time")?;
    rewind_for_test(p, n, 1);
    check(
        gettime(p, n) == Ok((0, 0)),
        "expired SIGEV_NONE one-shot reads 0",
    )?;
    check(
        settime(p, n, false, 1000 * SEC, 10 * MS).is_ok(),
        "SIGEV_NONE periodic",
    )?;
    rewind_for_test(p, n, 55 * MS);
    let (v, iv) = gettime(p, n).map_err(|_| KernelError::InternalError)?;
    check(
        iv == 10 * MS && v > 0 && v <= 10 * MS,
        "SIGEV_NONE periodic moves on",
    )?;
    check(getoverrun(p, n) == Ok(0), "and delivers no overrun")?;
    check(signal::pending(p) == 0, "SIGEV_NONE queues nothing")?;
    serial_println!("[posix_timer]   SIGEV_NONE: OK");

    // --- a real-time signal, an absolute wall-clock arming, a clock step ---
    let r =
        create(p, TimerClock::Realtime, sig(SIGRT, 7)).map_err(|_| KernelError::InternalError)?;
    let wall = Domain::Realtime.now();
    check(
        settime(p, r, true, wall.saturating_add(1000 * SEC), 0).is_ok(),
        "abs arm",
    )?;
    clock_was_set();
    check(armed_for_test(p, r), "still armed after a clock step")?;
    let (v, _) = gettime(p, r).map_err(|_| KernelError::InternalError)?;
    check(v > 999 * SEC && v <= 1000 * SEC, "abs reads ~1000s")?;
    check(expire_for_test(p, r), "abs expires")?;
    let got = signal::take_pending_info_in_mask(p, 1 << (SIGRT - 1));
    check(
        got.is_some_and(|(s, i)| s == SIGRT && is_timer_record(&i, r, 7)),
        "SIGRTMIN+2 record",
    )?;
    serial_println!("[posix_timer]   absolute wall-clock arming: OK");

    // --- exec deletes them all and keeps the id counter; exit forgets all ---
    check(
        settime(p, a, false, 1000 * SEC, 0).is_ok() && expire_for_test(p, a),
        "a queued",
    )?;
    let before = count(p);
    on_exec(p);
    check(before > 0 && count(p) == 0, "exec deletes every timer")?;
    check(signal::pending(p) == 0, "and their queued signals")?;
    let next = create(p, TimerClock::Monotonic, sig(SIGUSR1, 0))
        .map_err(|_| KernelError::InternalError)?;
    check(next > r, "the id counter survives exec")?;
    process_exit(p);
    check(count(p) == 0, "exit deletes every timer")?;
    signal::remove(p);
    serial_println!("[posix_timer]   exec / exit teardown: OK");

    serial_println!("[posix_timer] POSIX timer self-test PASSED (9 groups)");
    Ok(())
}
