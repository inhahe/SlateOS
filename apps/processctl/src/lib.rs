//! What a task manager does to a process it picked, and the line it says
//! about what happened.
//!
//! **Three ways to end one** (design-decisions §1072, the operator's answer
//! to B-Q22: "the same choices should also be available when killing via
//! the task manager"):
//!
//! | [`Ending`] | the program | Windows' Task Manager |
//! |---|---|---|
//! | [`Ending::Close`] | is asked what closing its window asks: it may ask to save first, and may decline | "End task" |
//! | [`Ending::End`] | is asked to stop now, saving what it can and asking nothing | (a session's end) |
//! | [`Ending::Force`] | is given no say: the system ends it | "End process" |
//!
//! Each goes through `procstop`, as `kill`, `pkill` and `killall` send theirs:
//! a program that answers SlateOS's requests is asked; any other is sent
//! `SIGTERM` for *end*, and cannot be asked to *close* at all -- which no
//! signal means -- so its line says to end it without asking instead, as
//! Windows offers "End process" for a program that does not respond.
//! *Force* is `SIGKILL` for every process.
//!
//! **Stop and continue** are the two signals no request means, `SIGSTOP` and
//! `SIGCONT`.
//!
//! All of it is asked of a [`procstop::Asker`]: `procstop::System::SINGLE`
//! for real -- whose `kill` refuses a PID of 0 or below, so a number read
//! wrongly never reaches the task manager's own process group, or every
//! process -- and `procstop::fake::Fake` in tests.
//!
//! **What is said is what was done**, never what it is hoped to cause: "Sent
//! end to X", "Asked X to close", not "Ended X". Whether a process goes is
//! its own business and the next refresh's news -- a program asked to close
//! may decline, `SIGTERM` can be caught, and even `SIGKILL` leaves a task in
//! the table until it is reaped. `apps/sysmonitor` and `apps/procexplorer`
//! both say them through this crate, so the two never word one outcome two
//! ways.

use libcall::{EINVAL, ENOSYS, EPERM};
use procstop::{ESRCH, Refusal, Request, Route, Via};

pub use libcall::{SIGCONT, SIGSTOP};
/// What a task manager names the system by, and what it asks of it --
/// re-exported so a window needs this crate alone.
pub use procstop::{Asker, SIGKILL, SIGTERM, System};

/// `procstop`'s fake system, for a window's tests (feature `fake`).
#[cfg(feature = "fake")]
pub use procstop::fake;

/// A way to end a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Ask it what closing its window asks; it may ask to save, or decline.
    Close,
    /// Ask it to stop now, asking nothing; `SIGTERM` if it cannot be asked.
    End,
    /// The system ends it: `SIGKILL`.
    Force,
}

impl Ending {
    /// Every way, gentlest first: the order a menu offers them in.
    pub const ALL: [Self; 3] = [Self::Close, Self::End, Self::Force];

    /// What a menu or a button calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Close => "Close",
            Self::End => "End Without Asking",
            Self::Force => "Force End",
        }
    }

    /// The verb a status line names it by: "Sent *end* to X".
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::End => "end",
            Self::Force => "force-end",
        }
    }

    /// The request `procstop` sends for it.
    #[must_use]
    pub const fn request(self) -> Request {
        match self {
            Self::Close => Request::Close,
            Self::End => Request::Terminate,
            Self::Force => Request::Force,
        }
    }
}

/// `pid` as `kill(2)` takes it, or `None` for one that is no single
/// process: 0, which `kill(2)` reads as the caller's own process group, and
/// any above `i32::MAX`, which would wrap to a negative -- a process group,
/// or every process. Refused here rather than left to the asker, so a
/// caller handing [`end`] an asker that does reach groups still cannot.
fn widen(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|&pid| pid > 0)
}

/// End process `pid` -- a PID from the process table -- the way `ending`
/// says, through `sys`: the route it went by.
///
/// # Errors
///
/// Why it could not be sent: `EINVAL` for a PID that does not fit, else
/// whatever `procstop::deliver` refused it for.
pub fn end<A: Asker + ?Sized>(sys: &A, pid: u32, ending: Ending) -> Result<Route, Refusal> {
    let pid = widen(pid).ok_or(Refusal::Errno(EINVAL))?;
    procstop::deliver(sys, pid, ending.request(), Via::Either)
}

/// Send signal `sig` -- [`SIGSTOP`], [`SIGCONT`], or 0 to ask whether the
/// process may be signalled at all -- to process `pid` through `sys`.
///
/// # Errors
///
/// `kill(2)`'s `errno`, or `EINVAL` for a PID that does not fit.
pub fn signal<A: Asker + ?Sized>(sys: &A, pid: u32, sig: i32) -> Result<(), i32> {
    sys.kill(widen(pid).ok_or(EINVAL)?, sig)
}

/// The status line for ending `name` (PID `pid`) the way `ending` says,
/// which `result` says how it went.
#[must_use]
pub fn ending_outcome(
    name: &str,
    pid: u32,
    ending: Ending,
    result: Result<Route, Refusal>,
) -> String {
    let verb = ending.verb();
    match result {
        Ok(Route::Message) if ending == Ending::Close => {
            format!("Asked {name} (PID {pid}) to close -- it may ask to save first")
        }
        Ok(Route::Message) => format!("Asked {name} (PID {pid}) to end without asking"),
        Ok(Route::Signal(_)) => format!("Sent {verb} to {name} (PID {pid})"),
        Err(Refusal::CannotClose) => format!(
            "{name} (PID {pid}) cannot be asked to close -- {} stops it",
            Ending::End.label()
        ),
        Err(Refusal::Errno(e)) => errno_outcome(name, pid, verb, e),
        // Each named, so a refusal procstop adds is a compile error here
        // rather than a line no one wrote for it. Each of these has a
        // sentence; only `Errno`, answered above, has none.
        Err(refusal @ (Refusal::NoAnswer | Refusal::Group | Refusal::NoCloseSignal)) => format!(
            "{name} (PID {pid}) was not sent {verb}: {}",
            refusal.sentence().unwrap_or("refused")
        ),
    }
}

/// End `name`, process `pid`, the way `ending` says, through `sys`: the
/// status line for what was done ([`end`], then [`ending_outcome`]).
#[must_use]
pub fn end_named<A: Asker + ?Sized>(sys: &A, name: &str, pid: u32, ending: Ending) -> String {
    ending_outcome(name, pid, ending, end(sys, pid, ending))
}

/// Send `sig` to `name`, process `pid`, through `sys`: the status line for
/// what was done, the signal called `verb` ([`signal`], then
/// [`signal_outcome`]).
#[must_use]
pub fn signal_named<A: Asker + ?Sized>(
    sys: &A,
    name: &str,
    pid: u32,
    sig: i32,
    verb: &str,
) -> String {
    signal_outcome(name, pid, verb, signal(sys, pid, sig))
}

/// The status line for sending `name` (PID `pid`) the signal a status line
/// calls `verb` ("stop", "continue"), which `result` says how it went.
#[must_use]
pub fn signal_outcome(name: &str, pid: u32, verb: &str, result: Result<(), i32>) -> String {
    match result {
        Ok(()) => format!("Sent {verb} to {name} (PID {pid})"),
        Err(e) => errno_outcome(name, pid, verb, e),
    }
}

/// What a failure's `errno` means to the user. `ESRCH` and `EPERM` are told
/// apart on purpose -- it is already gone, you are not allowed -- because
/// the user's next action differs completely between them.
fn errno_outcome(name: &str, pid: u32, verb: &str, errno: i32) -> String {
    match errno {
        ESRCH => format!("{name} (PID {pid}) is already gone -- nothing to {verb}"),
        EPERM => format!("Not permitted to {verb} {name} (PID {pid})"),
        ENOSYS => {
            format!("{name} (PID {pid}) was not sent {verb}: this system cannot signal processes")
        }
        EINVAL => format!("{name} (PID {pid}) is not a process this can signal"),
        e => format!("{name} (PID {pid}) was not sent {verb}: error {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use procstop::fake::fake;
    use procstop::{SIGKILL, SIGTERM};

    /// **Each way goes by the route its program understands**: a program
    /// that answers is asked to close or to end; one that does not is sent
    /// `SIGTERM` to end, and cannot be asked to close; force is `SIGKILL`
    /// for either.
    #[test]
    fn each_ending_goes_the_way_the_program_understands() {
        let sys = fake(&[5], &[]);
        assert_eq!(end(&sys, 5, Ending::Close), Ok(Route::Message));
        assert_eq!(end(&sys, 5, Ending::End), Ok(Route::Message));
        assert_eq!(end(&sys, 5, Ending::Force), Ok(Route::Signal(SIGKILL)));
        assert_eq!(end(&sys, 6, Ending::End), Ok(Route::Signal(SIGTERM)));
        assert_eq!(end(&sys, 6, Ending::Close), Err(Refusal::CannotClose));
        assert_eq!(end(&sys, 6, Ending::Force), Ok(Route::Signal(SIGKILL)));
        let did = sys.did.borrow();
        assert!(did.contains(&("close", 5, 0)), "{did:?}");
        assert!(did.contains(&("terminate", 5, 0)), "{did:?}");
        assert!(did.contains(&("kill", 6, SIGTERM)), "{did:?}");
    }

    /// **A PID that is no single process reaches nothing**: 0 is the
    /// caller's own process group to `kill(2)`, and one too big for it wraps
    /// to a negative -- a process group, or every process.
    #[test]
    fn a_pid_that_is_no_single_process_reaches_nothing() {
        let sys = fake(&[], &[]);
        let huge = 0x8000_0000_u32;
        assert_eq!(end(&sys, huge, Ending::Force), Err(Refusal::Errno(EINVAL)));
        assert_eq!(signal(&sys, huge, SIGSTOP), Err(EINVAL));
        // 0 is the caller's own process group to kill(2).
        assert_eq!(end(&sys, 0, Ending::Force), Err(Refusal::Errno(EINVAL)));
        assert_eq!(signal(&sys, 0, SIGSTOP), Err(EINVAL));
        assert!(sys.did.borrow().is_empty(), "something was sent");
        assert_eq!(signal(&sys, 7, SIGSTOP), Ok(()));
        assert_eq!(sys.did.borrow().last(), Some(&("kill", 7, SIGSTOP)));
    }

    /// **A process already gone says so**, by any way.
    #[test]
    fn a_process_already_gone_is_said_to_be() {
        let sys = fake(&[], &[9]);
        for ending in Ending::ALL {
            let said = ending_outcome("bash", 9, ending, end(&sys, 9, ending));
            assert_eq!(
                said,
                format!(
                    "bash (PID 9) is already gone -- nothing to {}",
                    ending.verb()
                ),
                "{ending:?}"
            );
        }
    }

    /// **What is said is what was done**: asked, or sent; never "closed" or
    /// "ended", which only the next refresh can tell. And the close a
    /// program cannot be asked names the way that stops it.
    #[test]
    fn the_line_says_what_was_done() {
        let said = |ending, r| ending_outcome("editor", 203, ending, r);
        assert_eq!(
            said(Ending::Close, Ok(Route::Message)),
            "Asked editor (PID 203) to close -- it may ask to save first"
        );
        assert_eq!(
            said(Ending::End, Ok(Route::Message)),
            "Asked editor (PID 203) to end without asking"
        );
        assert_eq!(
            said(Ending::End, Ok(Route::Signal(SIGTERM))),
            "Sent end to editor (PID 203)"
        );
        assert_eq!(
            said(Ending::Force, Ok(Route::Signal(SIGKILL))),
            "Sent force-end to editor (PID 203)"
        );
        assert_eq!(
            said(Ending::Close, Err(Refusal::CannotClose)),
            "editor (PID 203) cannot be asked to close -- End Without Asking stops it"
        );
        assert_eq!(
            said(Ending::End, Err(Refusal::Errno(EPERM))),
            "Not permitted to end editor (PID 203)"
        );
        assert_eq!(
            said(Ending::Force, Err(Refusal::Errno(ENOSYS))),
            "editor (PID 203) was not sent force-end: this system cannot signal processes"
        );
        assert_eq!(
            said(Ending::Force, Err(Refusal::Errno(EINVAL))),
            "editor (PID 203) is not a process this can signal"
        );
        assert_eq!(
            said(Ending::End, Err(Refusal::Errno(5))),
            "editor (PID 203) was not sent end: error 5"
        );
        assert_eq!(
            said(Ending::Close, Err(Refusal::NoAnswer)),
            "editor (PID 203) was not sent close: answers no requests"
        );
    }

    /// **A named ending and a named signal do it and say it**, in one call
    /// each, as both task managers ask for them.
    #[test]
    fn a_named_ending_or_signal_is_sent_and_said() {
        let sys = fake(&[], &[]);
        assert_eq!(
            end_named(&sys, "editor", 203, Ending::Force),
            "Sent force-end to editor (PID 203)"
        );
        assert_eq!(sys.did.borrow().last(), Some(&("kill", 203, SIGKILL)));
        assert_eq!(
            signal_named(&sys, "editor", 203, SIGSTOP, "stop"),
            "Sent stop to editor (PID 203)"
        );
        assert_eq!(sys.did.borrow().last(), Some(&("kill", 203, SIGSTOP)));
    }

    #[test]
    fn a_signals_line_says_what_was_done() {
        assert_eq!(
            signal_outcome("bash", 77, "stop", Ok(())),
            "Sent stop to bash (PID 77)"
        );
        assert_eq!(
            signal_outcome("bash", 77, "continue", Err(ESRCH)),
            "bash (PID 77) is already gone -- nothing to continue"
        );
    }

    /// **The ways come gentlest first, each named once.**
    #[test]
    fn the_ways_are_named_gentlest_first() {
        assert_eq!(Ending::ALL, [Ending::Close, Ending::End, Ending::Force]);
        let labels: Vec<_> = Ending::ALL.iter().map(|e| e.label()).collect();
        assert_eq!(labels, ["Close", "End Without Asking", "Force End"]);
        assert_eq!(Ending::Close.request(), Request::Close);
        assert_eq!(Ending::End.request(), Request::Terminate);
        assert_eq!(Ending::Force.request(), Request::Force);
    }
}
