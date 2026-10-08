//! Scheduling politeness ("nice"): who may change whose, and the three ways a
//! call names processes -- and a thread's scheduling policy, which is how a
//! thread becomes real-time ([`set_scheduler`], at the end).
//!
//! Nice runs from -20 (greediest) to 19 (most yielding).
//! [`thread::set_process_nice`](crate::proc::thread::set_process_nice) turns
//! it into a scheduler priority, and nice -20 is the top of the ordinary
//! levels (level 8, below only the real-time band), above every service.
//! Who may set it is therefore a security question, and it is decided here,
//! once, for every door:
//!
//! - the native `SYS_PROCESS_SET_NICE` (532, the caller's own), and
//!   `SYS_PROCESS_GET_PRIORITY` / `SYS_PROCESS_SET_PRIORITY` (1088/1089: a
//!   process, a group or a user's);
//! - the Linux `getpriority`, `setpriority` and `sched_setattr`.
//!
//! # The rule ([`may_set_nice`]; design-decisions §1503)
//!
//! Linux's `set_one_prio`, with this kernel's authority in place of uids:
//!
//! 1. **Authority over the target** ([`may_act_on`]): the target is the
//!    caller itself or the caller's child, the caller is a kernel task, or
//!    the caller holds a Process capability with DELETE rights for the
//!    target. That is exactly who may signal it
//!    (`syscall::handlers::check_signal_target`): a process may slow down
//!    the processes it may stop. Otherwise [`NiceRefusal::NotPermitted`],
//!    Linux's `EPERM`.
//! 2. **A raise** (a nice below the target's current one) must be within
//!    the target's `RLIMIT_NICE`, or the caller must hold a Thread
//!    capability with IO_REALTIME, which is the kernel's form of
//!    `CAP_SYS_NICE` (§326). Nice `n` needs a soft limit of `20 - n`, as in
//!    Linux's `can_nice`, so the default limit of 0 allows no raise at all.
//!    Otherwise [`NiceRefusal::RaiseRefused`], Linux's `EACCES`.
//!
//! Until 2026-10-01 neither half was the kernel's:
//! - the native call trusted libc to check raises, so any program could
//!   put itself above every service with one direct syscall;
//! - the Linux `setpriority` let any process renice any other;
//! - both acted on the caller when asked about a group or a user
//!   (`requests/e-ad-renicing-another-process-renices-the-caller.md`).
//!
//! Reading a nice needs no authority, as on Linux: `/proc/<pid>/stat` shows
//! it to anyone.
//!
//! # Real-time policies ([`may_set_scheduler`]; design-decisions 1544)
//!
//! `SCHED_FIFO` and `SCHED_RR` put a thread in the real-time band, levels
//! 0-7, above every ordinary thread (`sched::task::RT_LEVELS`). The same
//! authority is needed as for nice, and the same right lifts the same kind
//! of ceiling: a real-time policy needs a non-zero `RLIMIT_RTPRIO` (0 by
//! default) and stays within it, unless the caller holds IO_REALTIME, as
//! Linux's `CAP_SYS_NICE` lifts `RLIMIT_RTPRIO` there.

use alloc::vec::Vec;

use crate::cap::{ResourceType, Rights};
use crate::error::KernelError;
use crate::proc::pcb::{self, ProcessId};
use crate::sched::{
    self,
    task::{SchedAttr, SchedPolicy, TaskId},
};

/// `RLIMIT_NICE`'s number, as [`pcb::get_rlimit`] takes it.
const RLIMIT_NICE: u32 = 13;
const _: () = assert!(RLIMIT_NICE as usize == pcb::RLIMIT_NICE_INDEX);

/// How a priority call names processes: Linux's `PRIO_PROCESS`, `PRIO_PGRP`
/// and `PRIO_USER`, numbered as Linux numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrioWhich {
    /// One process; `who` is its pid.
    Process,
    /// Every live process of a process group; `who` is its pgid.
    Group,
    /// Every live process of a user; `who` is the uid.
    User,
}

impl PrioWhich {
    /// From a `which` argument: 0, 1 or 2.
    #[must_use]
    pub const fn from_raw(which: u64) -> Option<Self> {
        match which {
            0 => Some(Self::Process),
            1 => Some(Self::Group),
            2 => Some(Self::User),
            _ => None,
        }
    }
}

/// Why a nice change was refused, in the ways Linux's `set_one_prio` tells
/// apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NiceRefusal {
    /// Nothing is named: no such process (or a zombie), an empty group, a
    /// user with no processes. Linux's `ESRCH`.
    NoSuchProcess,
    /// The caller has no authority over a named process. Linux's `EPERM`.
    NotPermitted,
    /// A raise beyond the target's `RLIMIT_NICE`, by a caller without the
    /// right to raise priority. Linux's `EACCES`.
    RaiseRefused,
}

impl NiceRefusal {
    /// The native answer.
    ///
    /// The two refusals get different codes so that libc can give Linux's
    /// `EPERM` for the first and `EACCES` for the second:
    /// - no authority over the target answers `PermissionDenied`, as a
    ///   refused signal does;
    /// - a refused raise answers `ResourceExhausted` ("resource limit
    ///   reached"), because what refuses it is the `RLIMIT_NICE` ceiling,
    ///   which only the IO_REALTIME right lifts.
    #[must_use]
    pub const fn kernel_error(self) -> KernelError {
        match self {
            Self::NoSuchProcess => KernelError::NoSuchProcess,
            Self::NotPermitted => KernelError::PermissionDenied,
            Self::RaiseRefused => KernelError::ResourceExhausted,
        }
    }
}

/// Whether a soft `RLIMIT_NICE` of `soft` lets a process reach nice `nice`.
///
/// This is Linux's `can_nice` without its capability half. Nice `n` needs
/// `20 - n`, from 1 for nice 19 up to 40 for nice -20, so the default limit
/// of 0 allows nothing below the current nice. `RLIM_INFINITY` allows
/// everything.
#[must_use]
pub fn rlimit_allows_nice(soft: u64, nice: i32) -> bool {
    if soft == pcb::RLIM_INFINITY {
        return true;
    }
    // Clamped, so `20 - n` is 1..=40 and cannot overflow.
    let needed = 20i32.saturating_sub(nice.clamp(-20, 19)).unsigned_abs();
    u64::from(needed) <= soft
}

/// Whether `caller` has authority over `target`'s scheduling: it is the
/// caller itself, the caller's child, or a process the caller holds a
/// Process capability with DELETE rights for, or the caller is a kernel
/// task (pid 0). This is the authority a signal needs.
///
/// Answers [`NiceRefusal::NoSuchProcess`] for a target that is gone or a
/// zombie, and [`NiceRefusal::NotPermitted`] for a target the caller may
/// not touch.
pub fn may_act_on(caller: ProcessId, target: ProcessId) -> Result<(), NiceRefusal> {
    match pcb::state(target) {
        None | Some(pcb::ProcessState::Zombie) => return Err(NiceRefusal::NoSuchProcess),
        Some(_) => {}
    }
    if caller == 0 || target == caller {
        return Ok(());
    }
    let is_parent = pcb::parent(target) == Some(caller);
    let holds_cap = pcb::has_capability_for(caller, ResourceType::Process, target, Rights::DELETE);
    if is_parent || holds_cap {
        Ok(())
    } else {
        Err(NiceRefusal::NotPermitted)
    }
}

/// May `caller` set `target`'s nice to `nice` (clamped to -20..=19)? The
/// module's rule: authority over the target ([`may_act_on`]), then for a
/// raise the target's `RLIMIT_NICE` or the caller's IO_REALTIME right. A
/// kernel task may do anything.
pub fn may_set_nice(caller: ProcessId, target: ProcessId, nice: i32) -> Result<(), NiceRefusal> {
    may_act_on(caller, target)?;
    if caller == 0 {
        return Ok(());
    }
    let nice = nice.clamp(-20, 19);
    let current = pcb::get_nice(target).unwrap_or(0);
    if nice < current {
        let soft = pcb::get_rlimit(target, RLIMIT_NICE).map_or(0, |(soft, _)| soft);
        let may_raise = rlimit_allows_nice(soft, nice)
            || pcb::has_capability_type(caller, ResourceType::Thread, Rights::IO_REALTIME);
        if !may_raise {
            return Err(NiceRefusal::RaiseRefused);
        }
    }
    Ok(())
}

/// The live processes `which` and `who` name for `caller`; `who == 0` names
/// the caller's own process, group or user.
///
/// A zombie is not named: it has nothing left to schedule. A kernel task
/// (caller 0) has no process, group or user, so for it `who == 0` names
/// nothing; [`get_priority`] and [`set_priority`] answer that case
/// themselves.
#[must_use]
pub fn named_processes(caller: ProcessId, which: PrioWhich, who: u64) -> Vec<ProcessId> {
    match which {
        PrioWhich::Process => {
            let pid = if who == 0 { caller } else { who };
            match pcb::state(pid) {
                Some(state) if pid != 0 && state != pcb::ProcessState::Zombie => {
                    alloc::vec![pid]
                }
                _ => Vec::new(),
            }
        }
        PrioWhich::Group => {
            let pgid = if who == 0 {
                pcb::get_pgid(caller).unwrap_or(0)
            } else {
                who
            };
            if pgid == 0 {
                Vec::new()
            } else {
                pcb::pids_in_group(pgid)
            }
        }
        PrioWhich::User => {
            let uid = if who == 0 {
                pcb::process_uid(caller)
            } else {
                u32::try_from(who).ok()
            };
            uid.map_or_else(Vec::new, pcb::pids_of_user)
        }
    }
}

/// `getpriority`: the lowest nice, which is the highest priority, among the
/// processes `which` and `who` name. Linux reports the most favoured member
/// of a group the same way.
///
/// A kernel task asking about itself (`who == 0`) is told nice 0.
pub fn get_priority(caller: ProcessId, which: PrioWhich, who: u64) -> Result<i32, NiceRefusal> {
    if caller == 0 && who == 0 {
        return Ok(0);
    }
    named_processes(caller, which, who)
        .into_iter()
        .filter_map(pcb::get_nice)
        .min()
        .ok_or(NiceRefusal::NoSuchProcess)
}

/// `setpriority`: set the nice of every process `which` and `who` name to
/// `nice` (clamped to -20..=19), each one only if [`may_set_nice`] allows it.
///
/// The answer folds the members' outcomes as Linux's does:
/// - `NoSuchProcess` when nothing is named;
/// - otherwise the last refusal, if there was one;
/// - otherwise success.
///
/// A refusal for one member does not stop the others. A kernel task naming
/// itself (`who == 0`) has no nice to set, so that succeeds with no effect.
pub fn set_priority(
    caller: ProcessId,
    which: PrioWhich,
    who: u64,
    nice: i32,
) -> Result<(), NiceRefusal> {
    if caller == 0 && who == 0 {
        return Ok(());
    }
    let nice = nice.clamp(-20, 19);
    let mut outcome = Err(NiceRefusal::NoSuchProcess);
    for target in named_processes(caller, which, who) {
        match may_set_nice(caller, target, nice) {
            Ok(()) => {
                // A member that ended after it was named has nothing left to
                // set; it is skipped, as Linux skips a task that exits during
                // its walk.
                let set = crate::proc::thread::set_process_nice(target, nice).is_some();
                if set && outcome == Err(NiceRefusal::NoSuchProcess) {
                    outcome = Ok(());
                }
            }
            Err(refusal) => outcome = Err(refusal),
        }
    }
    outcome
}

/// Set process `pid`'s own nice, as `pid` asking for itself, and answer the
/// previous one. This is the native `SYS_PROCESS_SET_NICE`; a raise is
/// checked as for anyone else.
pub fn set_own_nice(pid: ProcessId, nice: i32) -> Result<i32, NiceRefusal> {
    may_set_nice(pid, pid, nice)?;
    crate::proc::thread::set_process_nice(pid, nice).ok_or(NiceRefusal::NoSuchProcess)
}

/// The nice a forked child's process starts with when the forking thread
/// had `SCHED_RESET_ON_FORK` (Linux's `sched_fork`): 0 if the thread was
/// real-time, otherwise its own with a negative one raised to 0.
#[must_use]
pub fn nice_after_reset_on_fork(creator_policy: SchedPolicy, nice: i32) -> i32 {
    if creator_policy.is_realtime() {
        0
    } else {
        nice.max(0)
    }
}

// ---------------------------------------------------------------------------
// Scheduling policies: who may make a thread real-time
// ---------------------------------------------------------------------------
//
// A thread's policy (`SCHED_OTHER`, `SCHED_FIFO`, `SCHED_RR`, `SCHED_BATCH`,
// `SCHED_IDLE`), its real-time priority and its `SCHED_RESET_ON_FORK` flag
// are its own (`sched::task::SchedAttr`), as on Linux; its nice is its
// process's. [`set_scheduler`] is the one place they are changed, for the
// Linux `sched_setscheduler`, `sched_setparam` and `sched_setattr` and the
// native `SYS_THREAD_SCHEDULER` alike (design-decisions 1544).

/// `RLIMIT_RTPRIO`'s number, as [`pcb::get_rlimit`] takes it.
const RLIMIT_RTPRIO: u32 = 14;
const _: () = assert!(RLIMIT_RTPRIO as usize == pcb::RLIMIT_RTPRIO_INDEX);

/// Why a change of a thread's scheduling was refused: Linux's answers for
/// `sched_setscheduler`, `sched_setparam` and `sched_setattr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedRefusal {
    /// No such thread. `ESRCH`.
    NoSuchThread,
    /// A priority outside the policy's range: 1..=99 under `SCHED_FIFO` and
    /// `SCHED_RR`, 0 under the others. `EINVAL`.
    Invalid,
    /// No authority over the thread, or a change only the right to raise
    /// priority allows. `EPERM`.
    NotPermitted,
}

/// What a call asks of a thread's scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedChange {
    /// The policy, or `None` to keep the thread's -- and with it its
    /// `SCHED_RESET_ON_FORK` -- as `sched_setparam` and
    /// `SCHED_FLAG_KEEP_POLICY` do.
    pub policy: Option<SchedPolicy>,
    /// The real-time priority asked for, as the caller passed it.
    pub rt_priority: i64,
    /// `SCHED_RESET_ON_FORK`, when `policy` is given.
    pub reset_on_fork: bool,
    /// `SCHED_FLAG_KEEP_PARAMS`: everything is checked as asked, but only
    /// the reset flag is stored, as Linux skips `__setscheduler_params`.
    pub keep_params: bool,
}

/// May `caller` move thread `tid`, now under `current`, to `wanted`?
/// Linux's `user_check_sched_setscheduler`, with this kernel's authority in
/// place of user ids and the IO_REALTIME right on a Thread capability in
/// place of `CAP_SYS_NICE`:
///
/// 1. **Authority over the thread's process** ([`may_act_on`]). No process
///    may change a kernel task, which belongs to none.
/// 2. **A change only the right allows** ([`needs_the_right`]) is refused
///    without it.
///
/// A kernel task (`caller` 0) may do anything.
pub fn may_set_scheduler(
    caller: ProcessId,
    tid: TaskId,
    current: SchedAttr,
    wanted: SchedAttr,
) -> Result<(), SchedRefusal> {
    if caller == 0 {
        return Ok(());
    }
    let owner = crate::proc::thread::owner_process(tid)
        .filter(|&p| p != 0)
        .ok_or(SchedRefusal::NotPermitted)?;
    match may_act_on(caller, owner) {
        Ok(()) => {}
        Err(NiceRefusal::NoSuchProcess) => return Err(SchedRefusal::NoSuchThread),
        Err(NiceRefusal::NotPermitted | NiceRefusal::RaiseRefused) => {
            return Err(SchedRefusal::NotPermitted);
        }
    }
    if needs_the_right(owner, current, wanted)
        && !pcb::has_capability_type(caller, ResourceType::Thread, Rights::IO_REALTIME)
    {
        return Err(SchedRefusal::NotPermitted);
    }
    Ok(())
}

/// Whether moving a thread of process `owner` from `current` to `wanted`
/// takes the right to raise priority -- each of Linux's `req_priv` cases:
///
/// - **a real-time policy** the thread is not already under, while
///   `owner`'s `RLIMIT_RTPRIO` is 0 (the default);
/// - **a real-time priority raised** past `RLIMIT_RTPRIO`;
/// - **leaving `SCHED_IDLE`** for a policy whose nice `owner`'s
///   `RLIMIT_NICE` would not let it raise itself to -- Linux counts
///   `SCHED_IDLE` as below nice 19, so coming back is a raise;
/// - **clearing `SCHED_RESET_ON_FORK`.**
///
/// Lowering never does: a real-time thread may always lower its priority or
/// become ordinary.
#[must_use]
pub fn needs_the_right(owner: ProcessId, current: SchedAttr, wanted: SchedAttr) -> bool {
    if wanted.policy.is_realtime() {
        let limit = pcb::get_rlimit(owner, RLIMIT_RTPRIO).map_or(0, |(soft, _)| soft);
        if wanted.policy != current.policy && limit == 0 {
            return true;
        }
        if wanted.rt_priority > current.rt_priority && u64::from(wanted.rt_priority) > limit {
            return true;
        }
    }
    if current.policy == SchedPolicy::Idle && wanted.policy != SchedPolicy::Idle {
        let soft = pcb::get_rlimit(owner, RLIMIT_NICE).map_or(0, |(soft, _)| soft);
        if !rlimit_allows_nice(soft, pcb::get_nice(owner).unwrap_or(0)) {
            return true;
        }
    }
    current.reset_on_fork && !wanted.reset_on_fork
}

/// The real-time priority `rt_priority` as a thread under `policy` holds it,
/// or [`SchedRefusal::Invalid`] if it does not suit the policy: Linux's
/// checks that a priority is at most `MAX_RT_PRIO - 1` (99) and that a
/// real-time policy has one (1..=99) and no other policy does (0). A
/// negative one is above 99 as Linux's `u32` field sees it.
pub fn check_priority(policy: SchedPolicy, rt_priority: i64) -> Result<u8, SchedRefusal> {
    let p = u8::try_from(rt_priority)
        .ok()
        .filter(|&p| p <= 99)
        .ok_or(SchedRefusal::Invalid)?;
    if policy.is_realtime() == (p != 0) {
        Ok(p)
    } else {
        Err(SchedRefusal::Invalid)
    }
}

/// Change thread `tid`'s scheduling as `caller` asks: Linux's
/// `__sched_setscheduler` for one thread, in its order -- no such thread
/// ([`SchedRefusal::NoSuchThread`]), then a priority out of the policy's
/// range ([`SchedRefusal::Invalid`]), then [`may_set_scheduler`].
///
/// A thread blocked on a priority-inheritance futex lends its new level on to
/// the holder ([`crate::ipc::futex::pi_waiter_priority_changed`]).
pub fn set_scheduler(
    caller: ProcessId,
    tid: TaskId,
    change: SchedChange,
) -> Result<(), SchedRefusal> {
    let current = sched::get_sched_attr(tid).ok_or(SchedRefusal::NoSuchThread)?;
    let policy = change.policy.unwrap_or(current.policy);
    let reset_on_fork = if change.policy.is_some() {
        change.reset_on_fork
    } else {
        current.reset_on_fork
    };
    let rt_priority = check_priority(policy, change.rt_priority)?;
    let wanted = SchedAttr {
        policy,
        rt_priority,
        reset_on_fork,
    };
    may_set_scheduler(caller, tid, current, wanted)?;
    let stored = if change.keep_params {
        SchedAttr {
            reset_on_fork,
            ..current
        }
    } else {
        wanted
    };
    sched::set_scheduler(tid, stored).ok_or(SchedRefusal::NoSuchThread)?;
    crate::ipc::futex::pi_waiter_priority_changed(tid);
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The rule, on scratch processes: a parent, its child, a stranger, and a
/// grandchild in the parent's group. The syscalls built on it are checked by
/// `syscall::dispatch`'s `test_dispatch_priority_doors`.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    fn fail(msg: &str, pids: &[ProcessId]) -> crate::error::KernelResult<()> {
        serial_println!("[priority]   FAIL: {}", msg);
        for &p in pids {
            pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // The rlimit arithmetic, at its edges.
    for (soft, nice, allowed) in [
        (0, 19, false),
        (1, 19, true),
        (20, 0, true),
        (20, -1, false),
        (21, -1, true),
        (40, -20, true),
        (39, -20, false),
        (pcb::RLIM_INFINITY, -20, true),
        (1000, -20, true),
    ] {
        if rlimit_allows_nice(soft, nice) != allowed {
            serial_println!(
                "[priority]   rlimit_allows_nice({}, {}) was not {}",
                soft,
                nice,
                allowed
            );
            return fail("the RLIMIT_NICE arithmetic", &[]);
        }
    }

    // Forked, so that the child and the grandchild are in the parent's group
    // (a created process leads a group and session of its own).
    let parent = pcb::create("prio-parent", 0);
    let stranger = pcb::create("prio-stranger", 0);
    let Ok(child) = pcb::fork_create(parent, 0, Vec::new(), Vec::new()) else {
        return fail("could not fork the scratch child", &[parent, stranger]);
    };
    let Ok(grandchild) = pcb::fork_create(child, 0, Vec::new(), Vec::new()) else {
        return fail(
            "could not fork the scratch grandchild",
            &[child, parent, stranger],
        );
    };
    let pids = [grandchild, child, parent, stranger];
    if pcb::pids_in_group(parent).len() != 3 {
        return fail("the scratch group is not parent, child, grandchild", &pids);
    }

    // Authority: itself, its child, not its grandchild, not a stranger.
    let checks = [
        (parent, parent, Ok(())),
        (parent, child, Ok(())),
        (parent, grandchild, Err(NiceRefusal::NotPermitted)),
        (parent, stranger, Err(NiceRefusal::NotPermitted)),
        (0, stranger, Ok(())),
        (parent, u64::MAX - 7, Err(NiceRefusal::NoSuchProcess)),
    ];
    for (caller, target, want) in checks {
        if may_act_on(caller, target) != want {
            serial_println!(
                "[priority]   may_act_on({}, {}) was not {:?}",
                caller,
                target,
                want
            );
            return fail("the authority rule", &pids);
        }
    }

    // Lowering a child's priority is allowed; raising it back is not, with
    // the default RLIMIT_NICE of 0 and no IO_REALTIME.
    if set_priority(parent, PrioWhich::Process, child, 10) != Ok(())
        || pcb::get_nice(child) != Some(10)
    {
        return fail("a parent could not lower its child's priority", &pids);
    }
    if set_priority(parent, PrioWhich::Process, child, 5) != Err(NiceRefusal::RaiseRefused)
        || pcb::get_nice(child) != Some(10)
    {
        return fail("a raise without the right went through", &pids);
    }
    // Its own raise is refused the same way.
    if set_own_nice(stranger, -5) != Err(NiceRefusal::RaiseRefused) {
        return fail("a process raised itself without the right", &pids);
    }
    // The target's RLIMIT_NICE: 25 allows down to nice -5, and no further.
    if pcb::set_rlimit(
        child,
        RLIMIT_NICE,
        25,
        25,
        pcb::LimitAuthority::MayRaiseHardLimit,
    )
    .is_err()
    {
        return fail("could not set the child's RLIMIT_NICE", &pids);
    }
    if set_priority(parent, PrioWhich::Process, child, -5) != Ok(())
        || set_priority(parent, PrioWhich::Process, child, -6) != Err(NiceRefusal::RaiseRefused)
        || pcb::get_nice(child) != Some(-5)
    {
        return fail("RLIMIT_NICE was not honoured as Linux's can_nice", &pids);
    }
    // IO_REALTIME on a Thread capability lifts the ceiling.
    if pcb::grant_capability(stranger, ResourceType::Thread, 0, Rights::IO_REALTIME).is_err() {
        return fail("could not grant IO_REALTIME", &pids);
    }
    if set_own_nice(stranger, -7) != Ok(0) || pcb::get_nice(stranger) != Some(-7) {
        return fail("IO_REALTIME did not permit a raise", &pids);
    }

    // A group: the parent may set itself and its child, not its grandchild.
    // Linux folds that to the refusal, having set the others.
    if set_priority(parent, PrioWhich::Group, parent, 3) != Err(NiceRefusal::NotPermitted)
        || pcb::get_nice(parent) != Some(3)
        || pcb::get_nice(grandchild) != Some(0)
    {
        return fail("a group renice did not fold as Linux's", &pids);
    }
    // `who == 0` is the caller's own group, and reading needs no authority:
    // the most favoured member is the grandchild, at nice 0.
    if get_priority(parent, PrioWhich::Group, 0) != Ok(0)
        || get_priority(parent, PrioWhich::Process, stranger) != Ok(-7)
    {
        return fail("getpriority's group minimum or its open read", &pids);
    }
    // A user: give the child and the grandchild a uid of their own.
    for p in [child, grandchild] {
        let creds = pcb::ProcessCredentials::new(4242, 4242);
        if pcb::set_credentials(p, creds).is_err() {
            return fail("could not give the scratch processes a user", &pids);
        }
    }
    // The child is at 3 since the group renice, the grandchild still at 0.
    if named_processes(parent, PrioWhich::User, 4242).len() != 2
        || get_priority(parent, PrioWhich::User, 4242) != Ok(0)
    {
        return fail("PRIO_USER did not name the user's processes", &pids);
    }
    // Nothing named is NoSuchProcess, for each form.
    for which in [PrioWhich::Process, PrioWhich::Group, PrioWhich::User] {
        if get_priority(parent, which, 0x7FFF_FFF0) != Err(NiceRefusal::NoSuchProcess)
            || set_priority(parent, which, 0x7FFF_FFF0, 1) != Err(NiceRefusal::NoSuchProcess)
        {
            return fail("an empty name was not NoSuchProcess", &pids);
        }
    }
    // A kernel task asking about itself: nice 0, and nothing to set.
    if get_priority(0, PrioWhich::Process, 0) != Ok(0)
        || set_priority(0, PrioWhich::Group, 0, 5) != Ok(())
    {
        return fail("a kernel task's own priority", &pids);
    }

    // Real-time policies, on a suspended thread counted as the child's.
    if let Err(why) = self_test_scheduler(parent, child, stranger) {
        return fail(why, &pids);
    }

    for p in pids {
        pcb::destroy(p);
    }
    serial_println!("[priority]   nice authority, raises, groups, users: OK");
    serial_println!("[priority]   real-time policies: authority, RLIMIT_RTPRIO, IO_REALTIME: OK");
    Ok(())
}

/// The real-time rule ([`may_set_scheduler`], [`needs_the_right`],
/// [`set_scheduler`]) on a suspended thread counted as `child`'s, which the
/// nice test above left at nice -5 under an `RLIMIT_NICE` of 25. `parent`
/// has authority over it; `stranger`, which holds IO_REALTIME by now, has
/// not.
fn self_test_scheduler(
    parent: ProcessId,
    child: ProcessId,
    stranger: ProcessId,
) -> Result<(), &'static str> {
    use crate::sched::task::rt_level;
    use SchedRefusal::{Invalid, NoSuchThread, NotPermitted};

    extern "C" fn never_runs(_arg: u64) {}

    let thread = sched::spawn_suspended(b"prio-rt", 16, never_runs, 0, 0)
        .map_err(|_| "could not spawn the scratch thread")?;
    let outcome = crate::proc::thread::self_test_with_thread(thread, child, || {
        let attr = |policy, rt_priority, reset_on_fork| SchedAttr {
            policy,
            rt_priority,
            reset_on_fork,
        };
        let other = SchedAttr::default();
        let idle = attr(SchedPolicy::Idle, 0, false);
        let fifo = |p| attr(SchedPolicy::Fifo, p, false);
        let change = |policy, rt_priority, reset_on_fork| SchedChange {
            policy,
            rt_priority,
            reset_on_fork,
            keep_params: false,
        };
        let now = || sched::get_sched_attr(thread);

        // Authority: the parent may, a stranger may not -- not even with
        // IO_REALTIME -- and the kernel always may. No process may change a
        // kernel task (the boot task here).
        let batch = attr(SchedPolicy::Batch, 0, false);
        if may_set_scheduler(stranger, thread, other, batch) != Err(NotPermitted)
            || may_set_scheduler(parent, thread, other, batch) != Ok(())
            || may_set_scheduler(0, thread, other, fifo(99)) != Ok(())
            || may_set_scheduler(parent, sched::current_task_id(), other, other)
                != Err(NotPermitted)
        {
            return Err("the authority over a thread");
        }
        // RLIMIT_RTPRIO 0, the default: no real-time policy without the right.
        if !needs_the_right(child, other, fifo(1))
            || may_set_scheduler(parent, thread, other, fifo(1)) != Err(NotPermitted)
        {
            return Err("a real-time policy under RLIMIT_RTPRIO 0");
        }
        // Lowering never needs it.
        if needs_the_right(child, fifo(50), fifo(10))
            || needs_the_right(child, fifo(50), other)
            || needs_the_right(child, fifo(50), idle)
        {
            return Err("lowering a real-time thread needed the right");
        }
        // RLIMIT_RTPRIO 30: in, and up to 30; a policy change within the
        // band; a priority already past it may stay or come down.
        if pcb::set_rlimit(
            child,
            RLIMIT_RTPRIO,
            30,
            30,
            pcb::LimitAuthority::MayRaiseHardLimit,
        )
        .is_err()
        {
            return Err("could not set the child's RLIMIT_RTPRIO");
        }
        if needs_the_right(child, other, fifo(30))
            || !needs_the_right(child, other, fifo(31))
            || needs_the_right(child, fifo(30), attr(SchedPolicy::Rr, 30, false))
            || !needs_the_right(child, fifo(10), fifo(31))
            || needs_the_right(child, fifo(60), fifo(60))
            || needs_the_right(child, fifo(60), fifo(50))
        {
            return Err("RLIMIT_RTPRIO's ceiling is not Linux's");
        }
        // SCHED_RESET_ON_FORK: setting it is free, clearing it is not.
        let reset = attr(SchedPolicy::Other, 0, true);
        if needs_the_right(child, other, reset) || !needs_the_right(child, reset, other) {
            return Err("SCHED_RESET_ON_FORK's rule");
        }
        // Leaving SCHED_IDLE is a raise to the process's nice: the child's
        // RLIMIT_NICE of 25 allows its nice of -5; the parent's 0 allows its
        // nice of 3 nothing.
        if needs_the_right(child, idle, other) || !needs_the_right(parent, idle, other) {
            return Err("leaving SCHED_IDLE is not judged by RLIMIT_NICE");
        }

        // set_scheduler: no such thread, then a bad priority (before who
        // may: a stranger's is Invalid too), then who may.
        if set_scheduler(
            parent,
            u64::MAX - 7,
            change(Some(SchedPolicy::Other), 0, false),
        ) != Err(NoSuchThread)
            || set_scheduler(parent, thread, change(Some(SchedPolicy::Fifo), 0, false))
                != Err(Invalid)
            || set_scheduler(parent, thread, change(Some(SchedPolicy::Other), 5, false))
                != Err(Invalid)
            || set_scheduler(parent, thread, change(Some(SchedPolicy::Fifo), 100, false))
                != Err(Invalid)
            || set_scheduler(parent, thread, change(Some(SchedPolicy::Rr), -1, false))
                != Err(Invalid)
            || set_scheduler(stranger, thread, change(Some(SchedPolicy::Fifo), 0, false))
                != Err(Invalid)
            || set_scheduler(stranger, thread, change(Some(SchedPolicy::Fifo), 5, false))
                != Err(NotPermitted)
        {
            return Err("set_scheduler's refusals or their order");
        }
        // What it stores: FIFO 20 with the flag, in the band at 20's level.
        if set_scheduler(parent, thread, change(Some(SchedPolicy::Fifo), 20, true)) != Ok(())
            || now() != Some(attr(SchedPolicy::Fifo, 20, true))
            || sched::get_base_priority(thread) != Some(rt_level(20))
        {
            return Err("FIFO 20 was not stored, or not at its level");
        }
        // No policy keeps the policy and the flag (sched_setparam).
        if set_scheduler(parent, thread, change(None, 25, false)) != Ok(())
            || now() != Some(attr(SchedPolicy::Fifo, 25, true))
        {
            return Err("keeping the policy did not keep it, or the flag");
        }
        // keep_params checks what it is given, and stores only the flag.
        let keep = SchedChange {
            policy: Some(SchedPolicy::Rr),
            rt_priority: 5,
            reset_on_fork: true,
            keep_params: true,
        };
        if set_scheduler(parent, thread, keep) != Ok(())
            || now() != Some(attr(SchedPolicy::Fifo, 25, true))
        {
            return Err("keep_params stored more than the flag");
        }
        // Clearing the flag takes the right, which the parent has not; an
        // ordinary policy with the flag kept is free.
        if set_scheduler(parent, thread, change(Some(SchedPolicy::Other), 0, false))
            != Err(NotPermitted)
            || set_scheduler(parent, thread, change(Some(SchedPolicy::Other), 0, true)) != Ok(())
            || now() != Some(reset)
            || sched::get_base_priority(thread) != Some(16)
        {
            return Err("the flag's clearing, or the return to the thread's level");
        }
        if set_scheduler(0, thread, change(Some(SchedPolicy::Other), 0, false)) != Ok(())
            || now() != Some(other)
        {
            return Err("the kernel could not clear the flag");
        }
        Ok(())
    });
    sched::kill_task(thread);
    sched::reap_dead_tasks();
    outcome
}
