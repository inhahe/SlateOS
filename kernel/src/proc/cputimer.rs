//! CPU-time timers that belong to a process as a whole -- `ITIMER_PROF` and
//! `ITIMER_VIRTUAL` (`setitimer`), and the `RLIMIT_CPU` limits -- and the
//! tick's entry point for every CPU-time timer, POSIX ones included.
//!
//! ## How a CPU-time timer fires
//!
//! A CPU-time timer expires when a clock that only advances while its process
//! (or thread) runs reaches a value, so there is no moment to program a timer
//! interrupt for. As on Linux, the timer tick checks: every tick charges the
//! task it interrupted ([`crate::sched::timer_tick`]), and compares the task's
//! sample with the earliest expiry armed on its own clocks
//! (`Task::cpu_timer_next`), and its process's running totals
//! ([`crate::sched::ProcCpuAccount`]) with the earliest expiry each source
//! armed on the process's clocks. Only when one is reached does it call
//! [`expire`], once the scheduler's lock is let go. So a CPU-time timer fires
//! at the first tick at or after its expiry: up to a tick (10 ms) late, never
//! early, which is Linux's resolution for them too.
//!
//! A process's running totals are kept only once a CPU-time timer has been
//! armed against it -- [`crate::proc::pcb::activate_cpu_account`], which fills
//! them from the process's clocks -- and from then on for the process's life.
//!
//! ## What is here
//!
//! - `ITIMER_PROF` and `ITIMER_VIRTUAL`: an expiry on the process's PROF or
//!   VIRT clock and a reload interval. At expiry the process is sent `SIGPROF`
//!   or `SIGVTALRM`, and the expiry moves on by one interval (or the timer is
//!   disarmed) -- one signal a tick at most, as Linux's `check_cpu_itimer`.
//!   Arming adds a tick to the value (Linux's `set_cpu_itimer`), so a timer
//!   never fires before its time has passed in whole ticks. Not inherited by
//!   `fork`; kept by `exec`.
//! - `RLIMIT_CPU`: at the soft limit of PROF time, `SIGXCPU`, again each
//!   second after (Linux raises the soft limit by a second each time, and so
//!   does this -- `getrlimit` shows it); at the hard limit, `SIGKILL`. An
//!   infinite soft limit turns both off, as on Linux.
//! - `RLIMIT_RTTIME`: the same for how long a real-time thread runs without
//!   blocking (`Task::rt_run_ticks`, counted at the tick while the soft limit
//!   is finite, cleared when it blocks), in microseconds.
//! - [`expire`], which also hands the tick's samples to
//!   [`crate::proc::posix_timer::expire_cpu`] for the POSIX timers on CPU
//!   clocks.
//!
//! ## Interrupt safety and lock order
//!
//! [`expire`] runs in the timer interrupt. It takes this module's [`TABLE`]
//! (with interrupts off, [`with_table`]), the POSIX timers' table, and the
//! signal registries -- all interrupt-safe -- and never the process table or
//! the scheduler's lock (it may try the latter). The process-context calls
//! read the process table *before* taking [`TABLE`], never inside it.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{self, ProcessId};
use crate::proc::signal::{self, SigInfo};
use crate::sched::{
    CpuClockKind, CpuTimerTick, NO_CPU_EXPIRY, PrevCputime, ProcCpuAccount, ProcExpiry, TICK_NS,
    adjust_cputime,
};
use crate::serial_println;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::Mutex;

/// Whose processor time a CPU-time clock reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuClockTarget {
    /// One thread's (a scheduler task id).
    Thread(crate::sched::task::TaskId),
    /// A whole process's: every thread it has, and every one it had.
    Process(ProcessId),
}

/// A CPU-time clock: a measure of a thread's or a process's processor time.
/// The Linux ABI names them by clock id (`syscall::linux::cpu_clock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuClock {
    /// What it measures.
    pub kind: CpuClockKind,
    /// Whose.
    pub target: CpuClockTarget,
}

impl CpuClock {
    /// The sample its target's clock reads from, or `None` if the target has
    /// gone: a thread's task reaped, or a process reaped. (A thread that has
    /// exited but whose task lingers still reads; the callers that must tell,
    /// ask [`Self::target_alive`].) Takes the process table and the
    /// scheduler's lock: process context.
    #[must_use]
    pub fn sample(self) -> Option<crate::sched::CpuSample> {
        match self.target {
            CpuClockTarget::Thread(tid) => crate::sched::cpu_sample(tid),
            CpuClockTarget::Process(pid) => crate::proc::thread::process_cpu_sample(pid),
        }
    }

    /// Whether the clock's target is still there for a timer of process
    /// `owner`'s: a thread still one of `owner`'s (it is no thread of any
    /// process once it has exited), or a process not yet reaped -- Linux's
    /// `cpu_timer_task_rcu`.
    #[must_use]
    pub fn target_alive(self, owner: ProcessId) -> bool {
        match self.target {
            CpuClockTarget::Thread(tid) => crate::proc::thread::owner_process(tid) == Some(owner),
            CpuClockTarget::Process(pid) => pcb::state(pid).is_some(),
        }
    }

    /// The clock's value in nanoseconds, or `None` if its target has gone.
    /// `CpuClockKind::Sched` reads the precise run time, PROF and VIRT the
    /// ticks, each worth a whole tick -- what Linux's read under tick
    /// accounting, so those move in steps of the tick as the itimers on them
    /// are charged.
    #[must_use]
    pub fn read(self) -> Option<u64> {
        self.sample().map(|s| s.value(self.kind))
    }

    /// Its resolution in nanoseconds -- Linux's `posix_cpu_clock_getres`: 1
    /// for `CPUCLOCK_SCHED`, a tick for the sampled ones.
    #[must_use]
    pub const fn resolution(self) -> u64 {
        match self.kind {
            CpuClockKind::Sched => 1,
            CpuClockKind::Prof | CpuClockKind::Virt => TICK_NS,
        }
    }
}

/// `ITIMER_VIRTUAL`: user time.
pub const ITIMER_VIRTUAL: u32 = 1;
/// `ITIMER_PROF`: user plus system time.
pub const ITIMER_PROF: u32 = 2;

/// `SIGKILL`.
const SIGKILL: u32 = 9;
/// `SIGXCPU`: the `RLIMIT_CPU` soft limit was reached.
const SIGXCPU: u32 = 24;
/// `SIGVTALRM`: `ITIMER_VIRTUAL` expired.
const SIGVTALRM: u32 = 26;
/// `SIGPROF`: `ITIMER_PROF` expired.
const SIGPROF: u32 = 27;

/// `RLIMIT_CPU`, in seconds.
pub const RLIMIT_CPU: u32 = 0;
/// `RLIMIT_RTTIME`, in microseconds a real-time thread may run without
/// blocking.
pub const RLIMIT_RTTIME: u32 = 15;
/// Microseconds per second.
const US_PER_SEC: u64 = 1_000_000;
/// `RLIM_INFINITY` (`RLIM64_INFINITY`).
const RLIM_INFINITY: u64 = u64::MAX;
/// Nanoseconds per second.
const NS_PER_SEC: u64 = 1_000_000_000;

/// One CPU-time itimer: Linux's `struct cpu_itimer`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CpuItimer {
    /// The expiry, on the process's clock of the itimer's measure, in ns;
    /// 0 when disarmed.
    expires: u64,
    /// The reload interval, in ns; 0 for a one-shot.
    incr: u64,
}

impl CpuItimer {
    /// Linux's `check_cpu_itimer`: if armed and `now` has reached the expiry,
    /// move the expiry on by one interval (or disarm) and answer `true` -- a
    /// signal is due.
    fn check(&mut self, now: u64) -> bool {
        if self.expires == 0 || now < self.expires {
            return false;
        }
        self.expires = if self.incr == 0 {
            0
        } else {
            self.expires.saturating_add(self.incr)
        };
        true
    }

    /// Its expiry as an expiry-slot value: [`NO_CPU_EXPIRY`] when disarmed.
    const fn slot(self) -> u64 {
        if self.expires == 0 {
            NO_CPU_EXPIRY
        } else {
            self.expires
        }
    }

    /// The time left, as `getitimer` reports it at `now` (Linux's
    /// `get_cpu_itimer`): 0 when disarmed, a tick when already due.
    const fn remaining(self, now: u64) -> u64 {
        if self.expires == 0 {
            0
        } else if self.expires < now {
            TICK_NS
        } else {
            self.expires.saturating_sub(now)
        }
    }
}

/// The `RLIMIT_CPU` thresholds, in ns of PROF time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuLimit {
    /// Where the next `SIGXCPU` falls: the soft limit, moved on a second per
    /// signal.
    soft: u64,
    /// Where `SIGKILL` falls: [`NO_CPU_EXPIRY`] for an infinite hard limit.
    hard: u64,
}

impl CpuLimit {
    /// Linux's `check_process_timers` for `RLIMIT_CPU` at PROF time `now`:
    /// `SIGKILL` at the hard limit (and nothing further), else `SIGXCPU` at
    /// the soft one, which then moves on a second.
    fn check(&mut self, now: u64) -> Option<u32> {
        // `NO_CPU_EXPIRY` is never -- an infinite limit, or one spent by its
        // SIGKILL -- not a threshold `now` reaches at `u64::MAX`, which the
        // self-test's "nothing after SIGKILL" asks and it did not hold.
        if self.hard != NO_CPU_EXPIRY && now >= self.hard {
            self.soft = NO_CPU_EXPIRY;
            self.hard = NO_CPU_EXPIRY;
            return Some(SIGKILL);
        }
        if self.soft != NO_CPU_EXPIRY && now >= self.soft {
            self.soft = self.soft.saturating_add(NS_PER_SEC);
            return Some(SIGXCPU);
        }
        None
    }

    /// The earliest threshold, as an expiry-slot value.
    fn slot(self) -> u64 {
        self.soft.min(self.hard)
    }
}

/// A process's CPU-time itimers and limits.
#[derive(Debug)]
struct Entry {
    /// Its running totals and expiry slots.
    account: Arc<ProcCpuAccount>,
    /// `ITIMER_PROF` (0) and `ITIMER_VIRTUAL` (1).
    itimer: [CpuItimer; 2],
    /// `RLIMIT_CPU`, if finite.
    limit: Option<CpuLimit>,
}

impl Entry {
    /// Publish this entry's earliest expiries to the account's slots.
    fn publish(&self) {
        let [prof, virt] = self.itimer;
        self.account.set_next(ProcExpiry::ItimerProf, prof.slot());
        self.account.set_next(ProcExpiry::ItimerVirt, virt.slot());
        self.account.set_next(
            ProcExpiry::RlimitCpu,
            self.limit.map_or(NO_CPU_EXPIRY, CpuLimit::slot),
        );
    }

    /// Whether it holds nothing, and can go.
    fn is_empty(&self) -> bool {
        self.itimer == [CpuItimer::default(); 2] && self.limit.is_none()
    }
}

/// Every process's CPU-time itimers and limits. Taken with interrupts off
/// ([`with_table`]): [`expire`] takes it in the timer interrupt.
static TABLE: Mutex<BTreeMap<ProcessId, Entry>> = Mutex::new(BTreeMap::new());

/// Run `f` with [`TABLE`] held and interrupts disabled.
fn with_table<R>(f: impl FnOnce(&mut BTreeMap<ProcessId, Entry>) -> R) -> R {
    crate::cpu::without_interrupts(|| f(&mut TABLE.lock()))
}

/// The itimer index and measure of `which`, or `None` for neither CPU one.
const fn itimer_of(which: u32) -> Option<(usize, CpuClockKind)> {
    match which {
        ITIMER_PROF => Some((0, CpuClockKind::Prof)),
        ITIMER_VIRTUAL => Some((1, CpuClockKind::Virt)),
        _ => None,
    }
}

/// `setitimer(ITIMER_PROF | ITIMER_VIRTUAL)` for process `pid`: arm the
/// itimer to expire after `value` ns of the process's PROF or VIRT time (and
/// every `interval` ns after), or disarm it if `value` is 0. Answers what it
/// held, as `getitimer` would have reported it -- Linux's `set_cpu_itimer`,
/// which also keeps a new interval on a disarmed itimer.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] for any other `which`;
/// [`KernelError::NoSuchProcess`] if there is no such process.
pub fn set_itimer(
    pid: ProcessId,
    which: u32,
    value: u64,
    interval: u64,
) -> KernelResult<(u64, u64)> {
    let (idx, kind) = itimer_of(which).ok_or(KernelError::InvalidArgument)?;
    // The totals are kept from here on: the old value is measured against
    // them, and the new expiry checked against them.
    let account = pcb::activate_cpu_account(pid).ok_or(KernelError::NoSuchProcess)?;
    let now = account.totals(0).value(kind);
    let removed = with_table(|table| {
        let entry = table.entry(pid).or_insert_with(|| Entry {
            account: Arc::clone(&account),
            itimer: [CpuItimer::default(); 2],
            limit: None,
        });
        let it = entry.itimer.get_mut(idx)?;
        let old = *it;
        // Linux's `set_process_cpu_timer`: an old expiry already reached
        // reads as a tick, a new value counts from now -- plus the tick
        // `set_cpu_itimer` adds, so it never fires before its time.
        let old_value = if old.expires == 0 {
            0
        } else if old.expires <= now {
            TICK_NS
        } else {
            old.expires.saturating_sub(now)
        };
        it.expires = if value == 0 {
            0
        } else {
            now.saturating_add(value).saturating_add(TICK_NS)
        };
        it.incr = interval;
        entry.publish();
        let answer = (old_value, old.incr);
        // An entry left holding nothing goes (outside the lock).
        let removed = if entry.is_empty() {
            table.remove(&pid)
        } else {
            None
        };
        Some((answer, removed))
    });
    let (answer, removed) = removed.ok_or(KernelError::InvalidArgument)?;
    drop(removed);
    Ok(answer)
}

/// `getitimer(ITIMER_PROF | ITIMER_VIRTUAL)` for process `pid`: the time to
/// the itimer's next expiry and its interval, in ns (Linux's
/// `get_cpu_itimer`: a disarmed itimer still reports the interval it was
/// given).
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] for any other `which`.
pub fn get_itimer(pid: ProcessId, which: u32) -> KernelResult<(u64, u64)> {
    let (idx, kind) = itimer_of(which).ok_or(KernelError::InvalidArgument)?;
    Ok(with_table(|table| {
        table.get(&pid).map_or((0, 0), |entry| {
            let it = entry.itimer.get(idx).copied().unwrap_or_default();
            (it.remaining(entry.account.totals(0).value(kind)), it.incr)
        })
    }))
}

/// Process `pid`'s `RLIMIT_CPU` changed (`setrlimit`, `prlimit`), or it was
/// just made with one (`fork` copies the parent's): take the limits up from
/// the process table and arm, re-arm or drop the thresholds -- Linux's
/// `update_rlimit_cpu`. A soft limit already passed is due at the next tick.
pub fn rlimit_cpu_changed(pid: ProcessId) {
    let Some((soft, hard)) = pcb::get_rlimit_stored(pid, RLIMIT_CPU) else {
        return;
    };
    let limit = (soft != RLIM_INFINITY).then(|| CpuLimit {
        soft: soft.saturating_mul(NS_PER_SEC),
        hard: if hard == RLIM_INFINITY {
            NO_CPU_EXPIRY
        } else {
            hard.saturating_mul(NS_PER_SEC)
        },
    });
    let account = if limit.is_some() {
        pcb::activate_cpu_account(pid)
    } else {
        pcb::cpu_account(pid)
    };
    let Some(account) = account else {
        return;
    };
    let removed = with_table(|table| {
        if limit.is_none() && !table.contains_key(&pid) {
            return None;
        }
        let entry = table.entry(pid).or_insert_with(|| Entry {
            account: Arc::clone(&account),
            itimer: [CpuItimer::default(); 2],
            limit: None,
        });
        entry.limit = limit;
        entry.publish();
        if entry.is_empty() {
            table.remove(&pid)
        } else {
            None
        }
    });
    drop(removed);
}

/// The tick found a CPU-time timer due for the task it interrupted, or for
/// its process (`sched::cpu_timers_due`): fire every one that is, against the
/// tick's samples. Interrupt context, with the scheduler's lock let go.
///
/// The POSIX timers first ([`crate::proc::posix_timer::expire_cpu`]); then,
/// against the process's running totals, its itimers and `RLIMIT_CPU`. Their
/// signals are sent once this module's lock is let go; a `SIGXCPU` also
/// raises the process's soft limit by a second, through the work queue
/// (the process table is no lock for an interrupt).
pub fn expire(due: CpuTimerTick) {
    crate::proc::posix_timer::expire_cpu(&due);
    if let (Some(ticks), Some(account)) = (due.rt_run_ticks, due.account.as_deref()) {
        rttime_expire(due.pid, account, ticks);
    }
    let Some(totals) = due.process else {
        return;
    };
    let mut signals = [0u32; 3];
    with_table(|table| {
        let Some(entry) = table.get_mut(&due.pid) else {
            return;
        };
        let [prof, virt] = &mut entry.itimer;
        if prof.check(totals.prof_ns()) {
            signals[0] = SIGPROF;
        }
        if virt.check(totals.virt_ns()) {
            signals[1] = SIGVTALRM;
        }
        if let Some(limit) = entry.limit.as_mut() {
            signals[2] = limit.check(totals.prof_ns()).unwrap_or(0);
        }
        entry.publish();
    });
    for sig in signals {
        if sig == 0 {
            continue;
        }
        // Process-directed, from the kernel (Linux's SEND_SIG_PRIV).
        signal::set_pending_info(due.pid, sig, SigInfo::kernel());
        if sig == SIGXCPU && !crate::workqueue::submit(raise_soft_limit, due.pid) {
            // The queue is full: the soft limit `getrlimit` shows lags the
            // next SIGXCPU's second, which is all that is lost.
            serial_println!(
                "[cputimer] work queue full: RLIMIT_CPU of {} not raised",
                due.pid
            );
        }
    }
}

/// A real-time thread of process `pid` has run `ticks` ticks without
/// blocking, past its `RLIMIT_RTTIME` (`account`): Linux's
/// `check_thread_timers` -- `SIGKILL` at the hard limit, else `SIGXCPU` at the
/// soft one, which moves on a second (and the process's soft limit with it,
/// through the work queue). Interrupt context.
fn rttime_expire(pid: ProcessId, account: &ProcCpuAccount, ticks: u64) {
    let (soft, hard) = account.rttime();
    let tick_us = TICK_NS.checked_div(1_000).unwrap_or(1);
    let ran_us = ticks.saturating_mul(tick_us);
    if hard != NO_CPU_EXPIRY && ran_us >= hard {
        signal::set_pending_info(pid, SIGKILL, SigInfo::kernel());
        return;
    }
    if soft != NO_CPU_EXPIRY && ran_us >= soft {
        account.set_rttime(soft.saturating_add(US_PER_SEC), hard);
        signal::set_pending_info(pid, SIGXCPU, SigInfo::kernel());
        if !crate::workqueue::submit(raise_rttime_soft_limit, pid) {
            serial_println!(
                "[cputimer] work queue full: RLIMIT_RTTIME of {} not raised",
                pid
            );
        }
    }
}

/// `stored` -- process `pid`'s limit `resource` as the process table holds
/// it -- with the soft limit as far as a `SIGXCPU` has raised it. Linux
/// raises `RLIMIT_CPU`'s and `RLIMIT_RTTIME`'s in the tick that sends the
/// signal, before a handler can ask; here the tick cannot take the process
/// table, which catches up through the work queue ([`raise_soft_limit`],
/// [`raise_rttime_soft_limit`]), and a reader in between -- the `SIGXCPU`
/// handler itself, as often as not -- is told the raised value
/// ([`pcb::get_rlimit`]). Every other limit, an infinite one, and one not
/// raised, is `stored`. Until 2026-10-08 the handler read the old limit (the
/// CPU-time ring-3 test, 0x7D).
#[must_use]
pub fn effective_limit(pid: ProcessId, resource: u32, stored: (u64, u64)) -> (u64, u64) {
    let (soft, hard) = stored;
    if soft == RLIM_INFINITY {
        return stored;
    }
    let raised = match resource {
        RLIMIT_CPU => with_table(|table| {
            table
                .get(&pid)
                .and_then(|entry| entry.limit)
                .map(|limit| limit.soft)
                .filter(|&next| next != NO_CPU_EXPIRY)
                .and_then(|next| next.checked_div(NS_PER_SEC))
        }),
        RLIMIT_RTTIME => pcb::cpu_account(pid)
            .map(|account| account.rttime().0)
            .filter(|&next| next != NO_CPU_EXPIRY),
        _ => None,
    };
    match raised {
        Some(next) if next > soft => (next.min(hard), hard),
        _ => stored,
    }
}

/// Work-queue half of an `RLIMIT_RTTIME` `SIGXCPU`: raise process `pid`'s
/// soft limit to where the account moved it, as Linux raises it.
fn raise_rttime_soft_limit(pid: u64) {
    let Some(account) = pcb::cpu_account(pid) else {
        return;
    };
    let (next, _) = account.rttime();
    if let Some((soft, hard)) = pcb::get_rlimit_stored(pid, RLIMIT_RTTIME)
        && soft != RLIM_INFINITY
        && next != NO_CPU_EXPIRY
        && soft < next
    {
        // Only up to the hard limit, which needs no authority.
        if pcb::set_rlimit(
            pid,
            RLIMIT_RTTIME,
            next.min(hard),
            hard,
            pcb::LimitAuthority::Unprivileged,
        )
        .is_err()
        {
            serial_println!(
                "[cputimer] RLIMIT_RTTIME of {} not raised: process gone",
                pid
            );
        }
    }
}

/// Process `pid`'s `RLIMIT_RTTIME` changed (`setrlimit`, `prlimit`), or it
/// was just made with one (`fork` copies the parent's): give the tick the
/// limits, in its account. An infinite soft limit turns the count off, as on
/// Linux.
pub fn rlimit_rttime_changed(pid: ProcessId) {
    let Some((soft, hard)) = pcb::get_rlimit_stored(pid, RLIMIT_RTTIME) else {
        return;
    };
    let Some(account) = pcb::cpu_account(pid) else {
        return;
    };
    let as_slot = |v: u64| if v == RLIM_INFINITY { NO_CPU_EXPIRY } else { v };
    account.set_rttime(as_slot(soft), as_slot(hard));
}

/// Work-queue half of a `SIGXCPU`: raise process `pid`'s `RLIMIT_CPU` soft
/// limit to where the next `SIGXCPU` falls -- a second past the last, as
/// Linux raises it at each one -- so `getrlimit` shows it. Taken from the
/// threshold the interrupt moved, not from the old limit plus one, so a
/// queue that runs late (two signals before it) neither lags nor moves the
/// threshold back.
fn raise_soft_limit(pid: u64) {
    let next_secs = with_table(|table| {
        table
            .get(&pid)
            .and_then(|entry| entry.limit)
            .filter(|limit| limit.soft != NO_CPU_EXPIRY)
            .and_then(|limit| limit.soft.checked_div(NS_PER_SEC))
    });
    let Some(next_secs) = next_secs else {
        return;
    };
    if let Some((soft, hard)) = pcb::get_rlimit_stored(pid, RLIMIT_CPU)
        && soft != RLIM_INFINITY
        && soft < next_secs
    {
        let raised = next_secs.min(hard);
        // The limit only moves up to the hard one, which needs no authority;
        // a process gone meanwhile has no limit to show.
        if pcb::set_rlimit(
            pid,
            RLIMIT_CPU,
            raised,
            hard,
            pcb::LimitAuthority::Unprivileged,
        )
        .is_err()
        {
            serial_println!("[cputimer] RLIMIT_CPU of {} not raised: process gone", pid);
        }
    }
}

/// Process teardown: forget its itimers and limits.
pub fn process_exit(pid: ProcessId) {
    let removed = with_table(|table| table.remove(&pid));
    drop(removed);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Log and fail unless `cond`.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[cputimer]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

/// The itimer and `RLIMIT_CPU` rules, on synthetic values: expiry and reload,
/// one signal a check however far past, the remaining time `getitimer`
/// reports, and the limits' `SIGXCPU` each second and `SIGKILL` at the hard
/// one. (The ring-3 tests drive the whole path from the tick.)
pub fn self_test() -> KernelResult<()> {
    const MS: u64 = 1_000_000;
    serial_println!("[cputimer] Running CPU-time itimer and limit self-test...");

    let mut it = CpuItimer {
        expires: 50 * MS,
        incr: 20 * MS,
    };
    check(!it.check(49 * MS), "an itimer before its expiry fired")?;
    check(it.remaining(49 * MS) == MS, "remaining before expiry")?;
    check(it.check(50 * MS), "an itimer at its expiry did not fire")?;
    check(
        it.expires == 70 * MS,
        "a periodic itimer did not move on one interval",
    )?;
    check(
        it.check(500 * MS),
        "a periodic itimer far past did not fire",
    )?;
    check(
        it.expires == 90 * MS,
        "an itimer far past moved on more than one interval",
    )?;
    check(
        it.remaining(100 * MS) == TICK_NS,
        "an itimer already due reads a tick",
    )?;
    let mut one = CpuItimer {
        expires: 10 * MS,
        incr: 0,
    };
    check(
        one.check(10 * MS) && one.expires == 0,
        "a one-shot itimer did not disarm",
    )?;
    check(
        !one.check(u64::MAX) && one.remaining(5) == 0,
        "a disarmed itimer fired",
    )?;
    check(
        one.slot() == NO_CPU_EXPIRY && it.slot() == 90 * MS,
        "itimer slots",
    )?;

    let mut limit = CpuLimit {
        soft: 2 * NS_PER_SEC,
        hard: 4 * NS_PER_SEC,
    };
    check(
        limit.check(NS_PER_SEC).is_none(),
        "RLIMIT_CPU fired below its soft limit",
    )?;
    check(
        limit.check(2 * NS_PER_SEC) == Some(SIGXCPU),
        "no SIGXCPU at the soft limit",
    )?;
    check(
        limit.soft == 3 * NS_PER_SEC,
        "the soft limit did not move on a second",
    )?;
    check(
        limit.check(2 * NS_PER_SEC + MS).is_none(),
        "a second SIGXCPU within the second",
    )?;
    check(
        limit.check(3 * NS_PER_SEC) == Some(SIGXCPU),
        "no SIGXCPU a second later",
    )?;
    check(
        limit.slot() == 4 * NS_PER_SEC,
        "the limit's slot is not its earliest threshold",
    )?;
    check(
        limit.check(4 * NS_PER_SEC) == Some(SIGKILL),
        "no SIGKILL at the hard limit",
    )?;
    check(
        limit.check(u64::MAX).is_none(),
        "the limits fired again after SIGKILL",
    )?;
    let mut soft_only = CpuLimit {
        soft: 0,
        hard: NO_CPU_EXPIRY,
    };
    check(
        soft_only.check(0) == Some(SIGXCPU),
        "a zero soft limit is not due at once",
    )?;

    // The user/system split of a run time (Linux's cputime_adjust).
    let mut prev = PrevCputime::default();
    check(
        adjust_cputime(30 * MS, 0, 0, &mut prev) == (30 * MS, 0),
        "a run time no tick saw is not all user",
    )?;
    let mut prev = PrevCputime::default();
    check(
        adjust_cputime(30 * MS, 0, 3, &mut prev) == (0, 30 * MS),
        "a run time only system ticks saw is not all system",
    )?;
    let mut prev = PrevCputime::default();
    check(
        adjust_cputime(30 * MS, 2, 1, &mut prev) == (20 * MS, 10 * MS),
        "a 2:1 run time is not split 2:1",
    )?;
    // The ratio swings to system: system rises, user holds where it was.
    check(
        adjust_cputime(33 * MS, 1, 10, &mut prev) == (20 * MS, 13 * MS),
        "a split went back as the ratio swung",
    )?;
    // No more run time: the last split again, whatever the ticks say.
    check(
        adjust_cputime(33 * MS, 100, 0, &mut prev) == (20 * MS, 13 * MS),
        "a split moved without more run time",
    )?;
    // Back to user: user rises, system holds.
    check(
        adjust_cputime(40 * MS, 100, 1, &mut prev) == (27 * MS, 13 * MS),
        "a split went back as the ratio swung back",
    )?;

    serial_println!("[cputimer] CPU-time itimer and limit self-test: OK");
    Ok(())
}
