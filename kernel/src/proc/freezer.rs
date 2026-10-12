//! The freezer: user threads stopped at the edge of user mode, and started
//! again where they stopped.
//!
//! A frozen thread is parked at a *signal-delivery checkpoint* -- the place a
//! system call, an interrupt or an exception passes on its way back to the
//! program ([`park_if_frozen`]) -- so when it is frozen nothing of the
//! kernel's is in progress on its behalf: it holds no lock, owns nothing on
//! its kernel stack but its saved registers, and is not half way through
//! anything. A thread waiting in an interruptible call (a read, a sleep, a
//! futex) is pulled out of it first: its wait sees [`interrupt_pending`], the
//! call returns the restart sentinel it returns for a signal, and since no
//! signal is delivered the checkpoint rewinds it, so that on the thaw the
//! program issues the same call again and never sees it end. Linux's freezer
//! works the same way (its "fake signal"); so does its cgroup v2 freezer.
//!
//! Two things freeze threads (design-decisions §1562):
//!
//! - **The whole system** ([`freeze_system`], [`thaw_system`]): every user
//!   thread but the caller's. This is the first step of replacing the running
//!   kernel without a restart and of hibernating (§1126): once
//!   [`freeze_system`] returns, every program is stopped at a point its
//!   registers alone describe.
//! - **A set of processes** ([`freeze_processes`], [`thaw_processes`]): a
//!   container's pause (`docker pause`), which used to suspend each thread
//!   wherever it was -- in the middle of kernel work holding a lock, say --
//!   and whose unpause resumed threads a `SIGSTOP` had stopped.
//!
//! A frozen thread can still be killed: a fatal signal ends the process where
//! it is, as cgroup v2 allows. Job control is untouched: a stopped thread
//! counts as frozen and stays stopped through the thaw, and a stop or continue
//! posted while frozen takes effect after it.
//!
//! ## Locking
//!
//! [`STATE`] is a leaf lock: nothing is called with it held but the
//! collections inside it. It is taken in process context only (the park, the
//! freeze and thaw, the wait loops' [`interrupt_pending`]), and on an
//! interrupt's way out to ring 3 -- where the interrupted code held no lock --
//! so it cannot be wanted by the code it interrupted.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{self, ProcessId};
use crate::sched::{self, task::TaskId, task::TaskState};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::wchan::{Wait, WaitChannel};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// The whole system is frozen ([`freeze_system`]).
static SYSTEM: AtomicBool = AtomicBool::new(false);

/// The thread that froze the system, which goes on running: it is the one
/// that thaws it, or (later) hands the frozen programs to a new kernel.
static EXEMPT: AtomicU64 = AtomicU64::new(0);

/// How many reasons to freeze are in force: 1 for the system, plus one per
/// frozen process. The fast path of every check is this one load.
static REASONS: AtomicUsize = AtomicUsize::new(0);

/// Threads parked, and processes frozen one by one.
struct FreezerState {
    /// Processes frozen by [`freeze_processes`] (a container's pause). Mirrored
    /// in [`FROZEN_PIDS`] for the lock-free check.
    processes: BTreeSet<ProcessId>,
    /// Every thread parked at a checkpoint now, with its process -- what the
    /// thaw wakes.
    parked: BTreeMap<TaskId, ProcessId>,
}

static STATE: Mutex<FreezerState> = Mutex::new(FreezerState {
    processes: BTreeSet::new(),
    parked: BTreeMap::new(),
});

/// How many processes can be frozen one by one at once: the slots of
/// [`FROZEN_PIDS`], a power of two.
pub const FROZEN_SLOTS: usize = 4096;

/// A slot that held a pid since thawed: probing goes on past it.
const TOMBSTONE: u64 = u64::MAX;

/// The processes frozen one by one, as an open-addressed set of atomic slots
/// (0 empty, [`TOMBSTONE`] vacated), so that [`interrupt_pending`] can ask
/// with no lock: it is asked inside wait loops, under the lock of whatever
/// object the thread waits on, where taking [`STATE`] -- a lock the lock
/// checker requires to be innermost of all -- is not allowed. Written only
/// under [`STATE`], beside [`FreezerState::processes`]; a reader racing a
/// writer may miss a process just frozen (the freezer wakes the waiter again
/// on its next pass) or see one just thawed (one spurious restart: the
/// checkpoint decides under the lock), never anything else.
static FROZEN_PIDS: [AtomicU64; FROZEN_SLOTS] = [const { AtomicU64::new(0) }; FROZEN_SLOTS];

/// The slot a probe for `pid` starts at.
fn home_slot(pid: ProcessId) -> usize {
    // Fibonacci hashing: pids are dense and sequential, and the multiply
    // spreads them over the table.
    let h = pid.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
    #[allow(clippy::cast_possible_truncation)] // masked to the table's size next
    let h = h as usize;
    h & (FROZEN_SLOTS - 1)
}

/// Whether `pid` is in [`FROZEN_PIDS`]. Lock-free.
fn frozen_pid_listed(pid: ProcessId) -> bool {
    let start = home_slot(pid);
    for i in 0..FROZEN_SLOTS {
        let slot = start.wrapping_add(i) & (FROZEN_SLOTS - 1);
        match FROZEN_PIDS.get(slot).map(|s| s.load(Ordering::Acquire)) {
            Some(0) | None => return false,
            Some(v) if v == pid => return true,
            Some(_) => {}
        }
    }
    false
}

/// Put `pid` in [`FROZEN_PIDS`]; `false` if every slot is taken. Under
/// [`STATE`], so writers never race each other.
fn list_frozen_pid(pid: ProcessId) -> bool {
    let start = home_slot(pid);
    let mut free = None;
    for i in 0..FROZEN_SLOTS {
        let slot = start.wrapping_add(i) & (FROZEN_SLOTS - 1);
        match FROZEN_PIDS.get(slot).map(|s| s.load(Ordering::Acquire)) {
            Some(v) if v == pid => return true,
            Some(TOMBSTONE) => {
                free.get_or_insert(slot);
            }
            Some(0) => {
                free.get_or_insert(slot);
                break;
            }
            _ => {}
        }
    }
    match free.and_then(|slot| FROZEN_PIDS.get(slot)) {
        Some(s) => {
            s.store(pid, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Take `pid` out of [`FROZEN_PIDS`]; when nothing is left frozen one by one,
/// clear the tombstones too. Under [`STATE`].
fn unlist_frozen_pid(pid: ProcessId, now_empty: bool) {
    if now_empty {
        for s in &FROZEN_PIDS {
            s.store(0, Ordering::Release);
        }
        return;
    }
    let start = home_slot(pid);
    for i in 0..FROZEN_SLOTS {
        let slot = start.wrapping_add(i) & (FROZEN_SLOTS - 1);
        let Some(s) = FROZEN_PIDS.get(slot) else {
            return;
        };
        match s.load(Ordering::Acquire) {
            0 => return,
            v if v == pid => {
                s.store(TOMBSTONE, Ordering::Release);
                return;
            }
            _ => {}
        }
    }
}

/// Counters for `/proc` and the self-test.
static FREEZES: AtomicU64 = AtomicU64::new(0);
static THAWS: AtomicU64 = AtomicU64::new(0);
static PARKS: AtomicU64 = AtomicU64::new(0);
static FAILED_FREEZES: AtomicU64 = AtomicU64::new(0);

/// Whether a thread of `pid`, `task`, must be frozen, given the processes
/// frozen one by one.
fn must_freeze(processes: &BTreeSet<ProcessId>, pid: ProcessId, task: TaskId) -> bool {
    if pid == 0 {
        return false;
    }
    (SYSTEM.load(Ordering::Acquire) && EXEMPT.load(Ordering::Acquire) != task)
        || processes.contains(&pid)
}

/// Whether anything is frozen or freezing at all: the one load every
/// checkpoint makes before anything else.
#[inline]
#[must_use]
pub fn any() -> bool {
    REASONS.load(Ordering::Acquire) != 0
}

/// Whether the calling thread, of process `pid`, is to freeze: what an
/// interruptible wait asks beside "is a signal pending?", so that it backs
/// out of the call -- with the result a signal would give it -- and the
/// thread reaches the checkpoint where it parks.
///
/// Lock-free, because it is asked under the lock of the object waited on:
/// the system flag, then [`FROZEN_PIDS`].
#[must_use]
pub fn interrupt_pending(pid: ProcessId) -> bool {
    if !any() || pid == 0 {
        return false;
    }
    let task = sched::current_task_id();
    (SYSTEM.load(Ordering::Acquire) && EXEMPT.load(Ordering::Acquire) != task)
        || frozen_pid_listed(pid)
}

/// Park the calling thread if it is to freeze, until it is thawed; `true` if
/// it parked. Called at the signal-delivery checkpoints, before a signal is
/// looked for -- as Linux's `get_signal` tries to freeze first -- with
/// interrupts enabled, on the thread's own kernel stack, holding nothing.
///
/// A thread killed while parked never comes back: the kill marks it dead
/// where it sleeps.
pub fn park_if_frozen() -> bool {
    if !any() {
        return false;
    }
    let task = sched::current_task_id();
    let Some(pid) = crate::proc::thread::owner_process(task) else {
        return false;
    };
    let mut parked = false;
    loop {
        {
            let mut st = STATE.lock();
            if !must_freeze(&st.processes, pid, task) {
                st.parked.remove(&task);
                break;
            }
            // Recorded before the block, so a thaw that comes between the two
            // finds the thread and wakes it; the wake is then the scheduler's
            // pending wake, and the block returns at once.
            st.parked.insert(task, pid);
        }
        if !parked {
            PARKS.fetch_add(1, Ordering::Relaxed);
        }
        parked = true;
        sched::block_current_on(Wait::FROZEN);
        // Any wake brings the thread here -- the thaw's, or a stray one -- and
        // the loop decides again.
    }
    parked
}

/// Whether `task` is parked by the freezer now.
#[must_use]
pub fn is_parked(task: TaskId) -> bool {
    // The guard goes at the end of this statement: the scheduler's lock is
    // never taken under `STATE`, which stays a leaf.
    let listed = any() && STATE.lock().parked.contains_key(&task);
    listed && sched::task_state(task) == Some(TaskState::Blocked)
}

/// A thread that would not freeze in time, and what it was doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Straggler {
    /// Its process.
    pub pid: ProcessId,
    /// The thread.
    pub task: TaskId,
    /// Its scheduler state when the freezer gave up.
    pub state: TaskState,
    /// What it was waiting on, if it was waiting.
    pub wait: Wait,
}

/// What became of one pass over the threads to freeze.
struct Pass {
    /// Threads parked, stopped by job control or never started.
    settled: usize,
    /// Threads not settled yet.
    stragglers: Vec<Straggler>,
}

/// Whether thread `task` is still to come to rest for the freezer: `None` if
/// it is at rest -- parked by the freezer, stopped (job control or a
/// debugger: `Suspended`), created and not yet admitted (it has run nothing,
/// and parks before its first instruction in ring 3), or gone -- and
/// otherwise its scheduler view.
fn unsettled(task: TaskId) -> Option<sched::RestView> {
    let view = sched::rest_view(task)?;
    let at_rest = match view.state {
        TaskState::Dead | TaskState::Suspended => true,
        TaskState::Blocked => view.wait.channel == WaitChannel::Frozen || view.awaiting_admission,
        TaskState::Ready | TaskState::Running => false,
    };
    (!at_rest).then_some(view)
}

/// One pass over the threads of the processes `filter` accepts: count those
/// at rest, wake those in an interruptible wait so that they back out, and
/// interrupt the CPUs running the rest, so that each passes a checkpoint.
fn pass(filter: &dyn Fn(ProcessId) -> bool, exempt: TaskId) -> Pass {
    let mut out = Pass {
        settled: 0,
        stragglers: Vec::new(),
    };
    for pid in pcb::pids() {
        if pid == 0 || !filter(pid) {
            continue;
        }
        let Some(threads) = pcb::get_threads(pid) else {
            continue;
        };
        let mut waiting = false;
        for task in threads {
            if task == exempt {
                continue;
            }
            match unsettled(task) {
                None => out.settled = out.settled.saturating_add(1),
                Some(view) => {
                    match view.state {
                        // In ring 3 or in a call: an interrupt brings it
                        // through a checkpoint either way.
                        TaskState::Running => sched::signal_cpu(view.last_cpu),
                        TaskState::Blocked => waiting = true,
                        // Ready: it passes a checkpoint when it next runs.
                        _ => {}
                    }
                    out.stragglers.push(Straggler {
                        pid,
                        task,
                        state: view.state,
                        wait: view.wait,
                    });
                }
            }
        }
        if waiting {
            // Exactly the waits a signal would end: each is registered as a
            // signal waiter while it is parked, and each re-checks on waking
            // -- where it now finds `interrupt_pending`.
            crate::proc::signal::wake_all_waiters(pid);
        }
    }
    out
}

/// What [`freeze_system`] and [`freeze_processes`] report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frozen {
    /// Threads at rest when it returned.
    pub threads: usize,
    /// Threads not at rest yet when it returned -- always 0 from a
    /// successful [`freeze_system`].
    pub pending: usize,
    /// How long it took, in nanoseconds.
    pub took_ns: u64,
}

/// Wait until every thread of the processes `filter` accepts is at rest, or
/// until `timeout_ns` has passed: the count either way, and the threads that
/// were not.
fn wait_settled(
    filter: &dyn Fn(ProcessId) -> bool,
    exempt: TaskId,
    timeout_ns: u64,
) -> (Frozen, Vec<Straggler>) {
    let start = crate::hrtimer::now_ns();
    let deadline = start.saturating_add(timeout_ns);
    loop {
        let p = pass(filter, exempt);
        let now = crate::hrtimer::now_ns();
        if p.stragglers.is_empty() || now >= deadline {
            let frozen = Frozen {
                threads: p.settled,
                pending: p.stragglers.len(),
                took_ns: now.saturating_sub(start),
            };
            return (frozen, p.stragglers);
        }
        sched::sleep_ms(1);
    }
}

/// Freeze every user thread but the caller's, waiting up to `timeout_ns` for
/// all of them to park.
///
/// On success every program is stopped at a checkpoint until
/// [`thaw_system`]. A thread that does not get there in time -- one in an
/// uninterruptible wait that does not end -- makes the freeze fail: the
/// system is thawed and the threads that would not freeze are returned, as
/// Linux reports "tasks refusing to freeze".
///
/// # Errors
///
/// `Err(None)` if the system is frozen already; `Err(Some(stragglers))` if it
/// could not be frozen in time.
pub fn freeze_system(timeout_ns: u64) -> Result<Frozen, Option<Vec<Straggler>>> {
    let me = sched::current_task_id();
    {
        let _st = STATE.lock();
        if SYSTEM.load(Ordering::Acquire) {
            return Err(None);
        }
        EXEMPT.store(me, Ordering::Release);
        SYSTEM.store(true, Ordering::Release);
        REASONS.fetch_add(1, Ordering::AcqRel);
    }
    FREEZES.fetch_add(1, Ordering::Relaxed);
    let (frozen, stragglers) = wait_settled(&|_| true, me, timeout_ns);
    if stragglers.is_empty() {
        return Ok(frozen);
    }
    FAILED_FREEZES.fetch_add(1, Ordering::Relaxed);
    thaw_system();
    Err(Some(stragglers))
}

/// End the system freeze: every thread parked for it goes on, unless its
/// process is frozen on its own account too. A no-op without one.
pub fn thaw_system() {
    let wake: Vec<TaskId> = {
        let st = STATE.lock();
        if !SYSTEM.load(Ordering::Acquire) {
            return;
        }
        SYSTEM.store(false, Ordering::Release);
        EXEMPT.store(0, Ordering::Release);
        REASONS.fetch_sub(1, Ordering::AcqRel);
        st.parked
            .iter()
            .filter(|(_, pid)| !st.processes.contains(pid))
            .map(|(task, _)| *task)
            .collect()
    };
    THAWS.fetch_add(1, Ordering::Relaxed);
    for task in wake {
        sched::wake(task);
    }
}

/// Add `pid` to the processes frozen one by one, in both records; `false` if
/// [`FROZEN_PIDS`] is full, with nothing changed.
fn add_frozen(st: &mut FreezerState, pid: ProcessId) -> bool {
    if pid == 0 || st.processes.contains(&pid) {
        return true;
    }
    if !list_frozen_pid(pid) {
        return false;
    }
    st.processes.insert(pid);
    REASONS.fetch_add(1, Ordering::AcqRel);
    true
}

/// Take `pid` out of the processes frozen one by one, in both records.
fn remove_frozen(st: &mut FreezerState, pid: ProcessId) {
    if st.processes.remove(&pid) {
        unlist_frozen_pid(pid, st.processes.is_empty());
        REASONS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Freeze the processes `pids` -- a container's pause -- waiting up to
/// `timeout_ns` for their threads to park. A process frozen already is left
/// as it is. Returns how many threads are at rest and how many are not yet:
/// the freeze stays in force either way (a straggler parks when it next
/// passes a checkpoint), as a cgroup's `cgroup.freeze` stays written while
/// `cgroup.events` says "not yet".
///
/// # Errors
///
/// `ResourceExhausted` if more than [`FROZEN_SLOTS`] processes would be
/// frozen one by one at once; none of `pids` is then frozen by this call.
pub fn freeze_processes(pids: &[ProcessId], timeout_ns: u64) -> KernelResult<Frozen> {
    {
        let mut st = STATE.lock();
        let mut added: Vec<ProcessId> = Vec::new();
        for &pid in pids {
            let fresh = pid != 0 && !st.processes.contains(&pid);
            if !add_frozen(&mut st, pid) {
                for done in added {
                    remove_frozen(&mut st, done);
                }
                return Err(KernelError::ResourceExhausted);
            }
            if fresh {
                added.push(pid);
            }
        }
    }
    FREEZES.fetch_add(1, Ordering::Relaxed);
    let set: BTreeSet<ProcessId> = pids.iter().copied().collect();
    Ok(wait_settled(&|pid| set.contains(&pid), 0, timeout_ns).0)
}

/// Thaw the processes `pids`: their threads go on, unless the system is
/// frozen. Processes not frozen are passed over.
pub fn thaw_processes(pids: &[ProcessId]) {
    let wake: Vec<TaskId> = {
        let mut st = STATE.lock();
        for &pid in pids {
            remove_frozen(&mut st, pid);
        }
        if SYSTEM.load(Ordering::Acquire) {
            Vec::new()
        } else {
            st.parked
                .iter()
                .filter(|(_, pid)| pids.contains(pid))
                .map(|(task, _)| *task)
                .collect()
        }
    };
    THAWS.fetch_add(1, Ordering::Relaxed);
    for task in wake {
        sched::wake(task);
    }
}

/// Whether process `pid` is frozen on its own account ([`freeze_processes`]).
#[must_use]
pub fn process_frozen(pid: ProcessId) -> bool {
    any() && STATE.lock().processes.contains(&pid)
}

/// Process `child` was just made by `parent` (fork, clone, spawn): a child
/// of a process frozen on its own account is frozen with it, as a cgroup's
/// new member is. (A system freeze covers it already.)
pub fn on_new_process(parent: ProcessId, child: ProcessId) {
    if !any() || child == 0 {
        return;
    }
    let mut st = STATE.lock();
    if st.processes.contains(&parent) && !add_frozen(&mut st, child) {
        // Every slot taken: the child runs. Said, since a paused container
        // with a running process in it is not paused.
        drop(st);
        crate::serial_println!(
            "[freezer] child {} of frozen process {} not frozen: {} processes are frozen already",
            child,
            parent,
            FROZEN_SLOTS
        );
    }
}

/// Process `pid` is gone: forget it.
pub fn on_process_exit(pid: ProcessId) {
    if !any() {
        return;
    }
    let mut st = STATE.lock();
    remove_frozen(&mut st, pid);
    st.parked.retain(|_, p| *p != pid);
}

/// The freezer's state for `/proc` and the kernel shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezerStats {
    /// The system is frozen now.
    pub system_frozen: bool,
    /// Processes frozen one by one now.
    pub frozen_processes: usize,
    /// Threads parked now.
    pub parked_threads: usize,
    /// Freezes asked for, of the system or of processes.
    pub freezes: u64,
    /// System freezes that failed and were undone.
    pub failed_freezes: u64,
    /// Thaws.
    pub thaws: u64,
    /// Times a thread parked.
    pub parks: u64,
}

/// The freezer's state now.
#[must_use]
pub fn stats() -> FreezerStats {
    let (frozen_processes, parked_threads) = {
        let st = STATE.lock();
        (st.processes.len(), st.parked.len())
    };
    FreezerStats {
        system_frozen: SYSTEM.load(Ordering::Acquire),
        frozen_processes,
        parked_threads,
        freezes: FREEZES.load(Ordering::Relaxed),
        failed_freezes: FAILED_FREEZES.load(Ordering::Relaxed),
        thaws: THAWS.load(Ordering::Relaxed),
        parks: PARKS.load(Ordering::Relaxed),
    }
}

/// Self-test of the bookkeeping that needs no program: a freeze of nothing,
/// the reasons count, a second system freeze refused, process freezes that
/// nest with it. The freezing of real threads is
/// `spawn::self_test_freezer` (ring 3).
///
/// # Errors
///
/// `InternalError` on the first failed check, after printing it.
pub fn self_test() -> KernelResult<()> {
    fn check(ok: bool, what: &str) -> KernelResult<()> {
        if ok {
            Ok(())
        } else {
            crate::serial_println!("[freezer]   FAIL: {}", what);
            Err(KernelError::InternalError)
        }
    }
    crate::serial_println!("[freezer] Running self-test...");
    let before = REASONS.load(Ordering::Acquire);
    check(before == 0, "something is frozen before the test")?;

    // A process that does not exist: frozen at once, nothing to wait for.
    const NOBODY: ProcessId = 0xFFFF_F2EE_0000;
    let Ok(r) = freeze_processes(&[NOBODY], 1_000_000) else {
        return check(false, "freezing one process found no room");
    };
    check(
        r.threads == 0 && r.pending == 0,
        "freezing a process with no threads did not succeed at once",
    )?;
    check(
        frozen_pid_listed(NOBODY) && !frozen_pid_listed(NOBODY + 2),
        "the lock-free set does not answer for the frozen process alone",
    )?;
    check(
        process_frozen(NOBODY),
        "the frozen process is not reported frozen",
    )?;
    check(any(), "nothing is reported frozen while a process is")?;
    // Its child is frozen with it; another's is not.
    on_new_process(NOBODY, NOBODY + 1);
    on_new_process(NOBODY + 7, NOBODY + 8);
    check(
        process_frozen(NOBODY + 1),
        "a frozen process's child is not frozen",
    )?;
    check(
        !process_frozen(NOBODY + 8),
        "an unfrozen process's child is frozen",
    )?;
    on_process_exit(NOBODY + 1);
    check(
        !process_frozen(NOBODY + 1),
        "an exited process is still frozen",
    )?;
    thaw_processes(&[NOBODY]);
    check(!process_frozen(NOBODY), "a thawed process is still frozen")?;
    check(
        REASONS.load(Ordering::Acquire) == 0,
        "the reasons count did not come back to zero",
    )?;
    // The kernel's own threads are never frozen.
    check(!interrupt_pending(0), "a kernel thread is told to freeze")?;

    // The lock-free set: full at FROZEN_SLOTS, one more refused with nothing
    // changed, a pid found past the tombstones its neighbours left, and every
    // slot empty again once nothing is frozen.
    let many: Vec<ProcessId> = (0..FROZEN_SLOTS as u64)
        .map(|i| NOBODY.wrapping_add(0x100).wrapping_add(i))
        .collect();
    check(
        freeze_processes(&many, 1_000_000).is_ok(),
        "the set did not take FROZEN_SLOTS processes",
    )?;
    check(
        freeze_processes(&[NOBODY], 1_000_000) == Err(KernelError::ResourceExhausted),
        "a full set took one more process",
    )?;
    check(
        !frozen_pid_listed(NOBODY) && !process_frozen(NOBODY),
        "a refused freeze froze its process",
    )?;
    let (odd, even): (Vec<ProcessId>, Vec<ProcessId>) = many.iter().partition(|&&p| p % 2 == 1);
    thaw_processes(&odd);
    check(
        even.iter().all(|&p| frozen_pid_listed(p)) && odd.iter().all(|&p| !frozen_pid_listed(p)),
        "the set lost a pid among tombstones, or kept a thawed one",
    )?;
    thaw_processes(&even);
    check(
        FROZEN_PIDS.iter().all(|s| s.load(Ordering::Acquire) == 0),
        "slots were left behind once nothing was frozen",
    )?;
    check(
        REASONS.load(Ordering::Acquire) == 0,
        "the reasons count did not come back to zero after the full set",
    )?;
    crate::serial_println!("[freezer]   bookkeeping and the lock-free set: OK");
    Ok(())
}
