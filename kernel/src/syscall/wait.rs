//! The one child-wait primitive, shared by every wait-shaped syscall.
//!
//! `wait4`, `waitid` and the native `SYS_PROCESS_WAIT_STATUS` all ask the
//! kernel the same question — "has one of my children changed state, and if
//! so which one and how?" — and all three read the *same* records: process
//! groups, zombie exit info, job-control reports. If they disagree about
//! which children a selector names, which transition is reported first, or
//! whether a report was consumed, then a program on our libc and a ported
//! glibc program waiting on the same child observe different histories.
//!
//! So the selection lives here exactly once. The callers differ only in how
//! they marshal the answer: `wait4` and the native syscall encode a POSIX
//! `wstatus` word, `waitid` encodes a `siginfo_t`.
//!
//! ## Why a target enum rather than a signed selector
//!
//! `waitpid`'s single `pid_t` overloads four meanings onto one integer
//! (`>0` a pid, `0` my group, `-1` any child, `<-1` group `-pid`), which
//! cannot name process group 1: that would need `-1`, already spoken for.
//! Linux avoids this by never overloading internally — `sys_waitid` resolves
//! its arguments to a `(type, struct pid *)` pair before calling `do_wait`.
//! [`WaitTarget`] is that pair. The `waitpid`-shaped callers convert into it
//! at the boundary with [`WaitTarget::from_posix_selector`], and callers that
//! can say what they mean (`waitid`'s `P_PGID`, and the native syscall's
//! `WPGID` option bit) construct [`WaitTarget::Pgid`] directly.
//!
//! ## Blocking discipline
//!
//! [`wait_for_child_event`] registers as a waiter *before* its final re-scan
//! and parks only after it, so a state change landing in the gap sets
//! `pending_wake` and is seen — there is no lost wakeup. The any-child waiter
//! flag lives on the *parent*, which survives reaping, so it is cleared on
//! every exit path; the specific-pid flag lives on the child, which is
//! destroyed when reaped, leaving nothing stale. The signal-waiter
//! registration bracketing the park is what lets a signal interrupt the wait,
//! and it is bounded to just the park so no early return can leak it.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{self, ExitInfo, JobControlEvent, ProcessId};
use crate::proc::thread::ProcessUsage;
use crate::sched;

/// Which children a wait is asking about.
///
/// Replaces the overloaded `waitpid` selector inside the kernel; see the
/// module docs for why. Every variant is total: there is no encoding a
/// caller can pass that means two things at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitTarget {
    /// Any child of the caller (`waitpid(-1)`, `waitid(P_ALL)`).
    Any,
    /// One specific child (`waitpid(pid)`, `waitid(P_PID)`).
    Pid(ProcessId),
    /// Any child in process group `pgid` (`waitpid(-pgid)`,
    /// `waitid(P_PGID)`, native `WPGID`). Unlike the `waitpid` encoding
    /// this can name group 1.
    Pgid(ProcessId),
}

impl WaitTarget {
    /// Convert a POSIX `waitpid`/`wait4` selector into a target.
    ///
    /// * `> 0` → [`WaitTarget::Pid`]
    /// * `0` → the caller's own process group
    /// * `-1` → [`WaitTarget::Any`]
    /// * `< -1` → group `-pid_arg`
    ///
    /// A caller with no group record — pid 0, i.e. the bare kernel tasks in
    /// the boot self-tests — yields [`WaitTarget::Any`] for the `0` case,
    /// which is the wait-any answer this path gave before group filtering
    /// existed.
    ///
    /// `i64::MIN` has no positive counterpart, so `unsigned_abs` yields
    /// 2^63: a well-defined `u64` that simply matches no group, so the
    /// caller sees `ECHILD` rather than a panic or a wrapped-around group
    /// id. (The Linux ABI rejects `INT_MIN` earlier with `ESRCH`, matching
    /// `kernel/exit.c`; that gate stays at the syscall boundary because it
    /// is an ABI detail, not a property of waiting.)
    #[must_use]
    pub fn from_posix_selector(pid_arg: i64, parent_pid: ProcessId) -> Self {
        if pid_arg > 0 {
            #[allow(clippy::cast_sign_loss)]
            WaitTarget::Pid(pid_arg as u64)
        } else if pid_arg == 0 {
            pcb::get_pgid(parent_pid).map_or(WaitTarget::Any, WaitTarget::Pgid)
        } else if pid_arg < -1 {
            WaitTarget::Pgid(pid_arg.unsigned_abs())
        } else {
            WaitTarget::Any
        }
    }

    /// The process-group restriction this target implies for an any-child
    /// scan, or `None` for an unrestricted scan. Meaningless for
    /// [`WaitTarget::Pid`], which names its child directly.
    fn pgid_filter(self) -> Option<ProcessId> {
        match self {
            WaitTarget::Pgid(g) => Some(g),
            WaitTarget::Any | WaitTarget::Pid(_) => None,
        }
    }
}

/// Which classes of state change a wait will accept.
///
/// `waitpid` always wants terminations and opts into the others with
/// `WUNTRACED`/`WCONTINUED`; `waitid` makes all three explicit
/// (`WEXITED`/`WSTOPPED`/`WCONTINUED`) and requires at least one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitClasses {
    /// Report a child that terminated (`WEXITED`; implicit in `waitpid`).
    pub exited: bool,
    /// Report a child stopped by a job-control signal (`WUNTRACED`/`WSTOPPED`).
    pub stopped: bool,
    /// Report a child resumed by `SIGCONT` (`WCONTINUED`).
    pub continued: bool,
}

impl WaitClasses {
    /// Whether either job-control class was requested.
    fn wants_job_control(self) -> bool {
        self.stopped || self.continued
    }
}

/// A complete wait request: who, what, and how to behave when there is
/// nothing to report yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitRequest {
    /// Which children are eligible.
    pub target: WaitTarget,
    /// Which state changes count.
    pub classes: WaitClasses,
    /// `WNOHANG`: return immediately instead of blocking.
    pub nohang: bool,
    /// `WNOWAIT`: report the state change but leave it unconsumed, so a
    /// later wait sees it again. A zombie is peeked instead of reaped; a
    /// job-control report is read without clearing it.
    pub nowait: bool,
}

/// What kind of transition was found.
#[derive(Debug, Clone)]
pub enum ChildEvent {
    /// The child terminated (normally, by signal, or by crash).
    Exited(ExitInfo),
    /// The child stopped or continued and is still alive.
    JobControl(JobControlEvent),
}

/// A child state change, in the neutral form both ABIs encode from.
///
/// Carries the facts that do not survive the reap — the child's real UID and
/// its resource usage — because a userspace caller cannot go back and ask
/// for them once the PCB is gone. That is the whole reason `waitid`'s
/// `si_uid` and `wait4`'s `rusage` have to be produced by the wait itself
/// rather than by a follow-up query.
#[derive(Debug, Clone)]
pub struct FoundEvent {
    /// The child whose state changed.
    pub pid: ProcessId,
    /// The child's real UID at the moment of the event (POSIX `si_uid`).
    pub uid: u32,
    /// The child's resource usage including its own reaped descendants
    /// (Linux `RUSAGE_BOTH`), snapshotted before any reap.
    pub usage: ProcessUsage,
    /// The transition itself.
    pub event: ChildEvent,
}

impl FoundEvent {
    /// Encode as a POSIX `wstatus` word (`wait4`, `SYS_PROCESS_WAIT_STATUS`).
    ///
    /// Delegates to the encoders on [`ExitInfo`] / [`JobControlEvent`], which
    /// is where the bit layout lives so no ABI can drift from another.
    #[must_use]
    pub fn to_wstatus(&self) -> i32 {
        match &self.event {
            ChildEvent::Exited(info) => info.to_wstatus(),
            ChildEvent::JobControl(ev) => ev.to_wstatus(),
        }
    }
}

/// One non-blocking pass of the child-selection logic.
///
/// Returns:
/// * `Ok(Some(found))` — a matching transition, consumed unless
///   `req.nowait`;
/// * `Ok(None)` — eligible children exist, but none has a matching
///   unconsumed transition: the caller should block (or answer a `WNOHANG`
///   miss);
/// * `Err(NoChildProcess | NoSuchProcess | PermissionDenied)` — no eligible
///   child at all. All three mean `ECHILD` to a POSIX caller: "not your
///   child" and "no such child" are deliberately indistinguishable so a
///   caller cannot probe for the existence of other people's processes.
///
/// Terminations are checked before job-control reports. That order is
/// observable — a child that stopped and then exited before its parent
/// looked is reported as exited, and the stop is lost — and it matches
/// Linux, whose `wait_consider_task` tests `EXIT_ZOMBIE` first.
pub fn scan_once(parent_pid: ProcessId, req: WaitRequest) -> KernelResult<Option<FoundEvent>> {
    let want_jc = req.classes.wants_job_control();

    if let WaitTarget::Pid(child) = req.target {
        if req.classes.exited {
            if let Some((info, uid)) = pcb::peek_exit(parent_pid, child)? {
                // Snapshot usage while the PCB still exists: `try_reap`
                // destroys it and folds these numbers into the parent.
                let usage = crate::proc::thread::process_usage_both(child);
                if !req.nowait {
                    pcb::try_reap(parent_pid, child)?;
                }
                return Ok(Some(FoundEvent {
                    pid: child,
                    uid,
                    usage,
                    event: ChildEvent::Exited(info),
                }));
            }
        }
        if want_jc {
            if let Some(ev) = pcb::jc_report_for_child(
                parent_pid,
                child,
                req.classes.stopped,
                req.classes.continued,
                !req.nowait,
            )? {
                return Ok(Some(FoundEvent {
                    pid: child,
                    uid: pcb::process_uid(child).unwrap_or(0),
                    usage: crate::proc::thread::process_usage_both(child),
                    event: ChildEvent::JobControl(ev),
                }));
            }
        }
        return Ok(None);
    }

    let pgid_filter = req.target.pgid_filter();
    if req.classes.exited {
        let peek = match pgid_filter {
            Some(g) => pcb::peek_exit_group(parent_pid, g),
            None => pcb::peek_exit_any(parent_pid),
        };
        match peek {
            Ok(Some((cpid, info, uid))) => {
                let usage = crate::proc::thread::process_usage_both(cpid);
                if !req.nowait {
                    pcb::try_reap(parent_pid, cpid)?;
                }
                return Ok(Some(FoundEvent {
                    pid: cpid,
                    uid,
                    usage,
                    event: ChildEvent::Exited(info),
                }));
            }
            Ok(None) => {}
            Err(e) => {
                // "No eligible child" from the exit scan. If a job-control
                // class was also requested, fall through to its scan, which
                // reaches the same conclusion by the same rule; otherwise
                // surface ECHILD now.
                if !want_jc {
                    return Err(e);
                }
            }
        }
    }
    if want_jc {
        let jc = match pgid_filter {
            Some(g) => pcb::jc_report_group(
                parent_pid,
                g,
                req.classes.stopped,
                req.classes.continued,
                !req.nowait,
            ),
            None => pcb::jc_report_any_child(
                parent_pid,
                req.classes.stopped,
                req.classes.continued,
                !req.nowait,
            ),
        };
        if let Some((cpid, ev)) = jc? {
            return Ok(Some(FoundEvent {
                pid: cpid,
                uid: pcb::process_uid(cpid).unwrap_or(0),
                usage: crate::proc::thread::process_usage_both(cpid),
                event: ChildEvent::JobControl(ev),
            }));
        }
    }
    Ok(None)
}

/// What a wait observed. Returned by [`wait_for_child_event`].
pub enum WaitOutcome {
    /// A child changed state.
    Changed(FoundEvent),
    /// `WNOHANG` was requested and no child was ready.
    NoHang,
    /// A deliverable signal arrived before the caller parked. The caller
    /// must return `restart::restart_result(ERESTARTSYS)`: the
    /// signal-delivery checkpoint runs the handler and then either restarts
    /// the wait (`SA_RESTART`) or substitutes `EINTR`.
    Restart,
}

/// Wait for a child state change, blocking unless `WNOHANG` was asked for.
///
/// The shared body of every wait-shaped syscall — see the module docs for
/// why there is only one, and for the blocking discipline this implements.
pub fn wait_for_child_event(
    parent_pid: ProcessId,
    task_id: u64,
    req: WaitRequest,
) -> KernelResult<WaitOutcome> {
    use crate::proc::signal;

    if let WaitTarget::Pid(child_pid) = req.target {
        loop {
            if let Some(found) = scan_once(parent_pid, req)? {
                return Ok(WaitOutcome::Changed(found));
            }
            if req.nohang {
                return Ok(WaitOutcome::NoHang);
            }
            // Only handler-backed signals can be pending-and-deliverable at
            // a park point (`classify_post` drops no-handler default-ignore
            // signals and terminates on fatal ones), so a restart can never
            // spuriously livelock here.
            let deliverable = !signal::blocked(parent_pid);
            if signal::has_pending_in_mask(parent_pid, deliverable) {
                return Ok(WaitOutcome::Restart);
            }
            pcb::set_wait_task(child_pid, task_id)?;
            if let Some(found) = scan_once(parent_pid, req)? {
                return Ok(WaitOutcome::Changed(found));
            }
            signal::register_signalfd_waiter(parent_pid, task_id, deliverable);
            if signal::has_pending_in_mask(parent_pid, deliverable) {
                signal::deregister_signalfd_waiter(parent_pid, task_id);
                continue;
            }
            sched::block_current();
            signal::deregister_signalfd_waiter(parent_pid, task_id);
        }
    } else {
        loop {
            // Registering *before* the scan closes the lost-wakeup window: a
            // child that changes state during the scan below still finds a
            // waiter to wake.
            //
            // Registration failing, however, is not an answer about children,
            // and the code this was lifted from treated it as one. It fails
            // exactly when the *caller* has no process record — a bare kernel
            // task, whose children are the real `parent == 0` rows in the
            // table. Returning that error here made every any-child and
            // every group wait from such a caller report ECHILD without ever
            // scanning. `scan_once` is what knows whether an eligible child
            // exists, so let it answer first; only a caller that actually has
            // to *park* is defeated by having no slot to be woken through.
            let registered = match pcb::set_wait_any_task(parent_pid, task_id) {
                Ok(()) => true,
                Err(KernelError::NoSuchProcess) => false,
                Err(e) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    return Err(e);
                }
            };
            // `clear_wait_any_task` is a no-op when the process is gone or the
            // slot holds another task, so the unconditional clears below stay
            // correct when `registered` is false.
            match scan_once(parent_pid, req) {
                Ok(Some(found)) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    return Ok(WaitOutcome::Changed(found));
                }
                Ok(None) => {}
                Err(e) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    return Err(e);
                }
            }
            if req.nohang {
                pcb::clear_wait_any_task(parent_pid, task_id);
                return Ok(WaitOutcome::NoHang);
            }
            if !registered {
                // Past this point the caller would park, and nothing can wake
                // a task that has no slot to be woken through — parking would
                // be an unconditional hang. Surface the registration failure
                // instead, which `wait_err_to_echild` folds into ECHILD at the
                // syscall boundary. Unreachable from userspace, where
                // `caller_pid()` always names a live process; reachable from a
                // kernel task, which is why it is an error and not a panic.
                return Err(KernelError::NoSuchProcess);
            }
            let deliverable = !signal::blocked(parent_pid);
            if signal::has_pending_in_mask(parent_pid, deliverable) {
                pcb::clear_wait_any_task(parent_pid, task_id);
                return Ok(WaitOutcome::Restart);
            }
            signal::register_signalfd_waiter(parent_pid, task_id, deliverable);
            if signal::has_pending_in_mask(parent_pid, deliverable) {
                signal::deregister_signalfd_waiter(parent_pid, task_id);
                continue;
            }
            sched::block_current();
            signal::deregister_signalfd_waiter(parent_pid, task_id);
        }
    }
}

/// Fold a scan/wait error into the single errno POSIX gives every
/// "no eligible child" case.
///
/// `ECHILD` covers "you have no children", "that is not your child" and
/// "there is no such process" alike, so a caller cannot use `waitpid` to
/// discover whether a PID it does not own exists.
#[must_use]
pub fn wait_err_to_echild(e: KernelError) -> KernelError {
    match e {
        KernelError::PermissionDenied | KernelError::NoSuchProcess => KernelError::NoChildProcess,
        other => other,
    }
}
