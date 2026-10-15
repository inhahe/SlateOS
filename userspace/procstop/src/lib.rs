//! How SlateOS stops a process (design-decisions §1072, the operator's
//! answer to B-Q22): three requests, each sent the way the process
//! understands it.
//!
//! | [`Request`] | a program that answers is | any other process is sent |
//! |---|---|---|
//! | [`Request::Close`] | asked to close, as its own close command does: it may ask the user to save, and may decline | nothing -- no signal means close; [`Refusal::CannotClose`] |
//! | [`Request::Terminate`] | asked to stop now and cleanly, asking nothing | `SIGTERM`, as on Linux |
//! | [`Request::Force`] | sent `SIGKILL` | `SIGKILL` |
//!
//! A program *answers* when it has registered a lifecycle endpoint
//! (`libservicebus::lifecycle`). Every program ported from Linux does not,
//! and a process group -- which no message reaches -- is only ever
//! signalled. [`Via`] insists on one route.
//!
//! [`deliver`] sends one request; [`escalate`] sends the next to whatever is
//! still there after a wait (`kill --timeout`). Both work through an
//! [`Asker`], which [`System`] is for real and the `fake` feature's
//! `fake::Fake` is for tests. `kill`, `pkill` and `killall` share this
//! (their options are `coreutils::procps::requests`), and so can anything
//! else that stops programs -- the task manager, a service manager.
//!
//! Off SlateOS no program answers, so a terminate is always `SIGTERM`.

use libcall::pty::{EIO, ENOMEM};
use libcall::{EAGAIN, EINVAL, ENOSYS, EPERM};
use libservicebus::{BusError, lifecycle};

// `SIGTERM`, which *terminate* is to a program that does not answer;
// `SIGKILL`, which *force* is; `ESRCH`, a process that is not there.
pub use libcall::{ESRCH, SIGKILL, SIGTERM};

/// How long [`escalate`] waits between looks at whether a process is still
/// there.
pub const TIMEOUT_POLL_MS: u64 = 20;

/// What is asked of a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// What its own close command does; it may decline.
    Close,
    /// Stop now, cleanly, asking nothing.
    Terminate,
    /// The kernel ends it: `SIGKILL`.
    Force,
}

impl Request {
    /// The request a word names: `close`, `terminate` or `force`.
    #[must_use]
    pub fn named(word: &[u8]) -> Option<Self> {
        match word {
            b"close" => Some(Self::Close),
            b"terminate" => Some(Self::Terminate),
            b"force" => Some(Self::Force),
            _ => None,
        }
    }
}

/// Which way a request may go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Via {
    /// The message if the program answers it, else the signal.
    #[default]
    Either,
    /// Only the message.
    Message,
    /// Only the signal.
    Signal,
}

impl Via {
    /// The route a word names: `message` or `signal`.
    #[must_use]
    pub fn named(word: &[u8]) -> Option<Self> {
        match word {
            b"message" => Some(Self::Message),
            b"signal" => Some(Self::Signal),
            _ => None,
        }
    }
}

/// The way a request went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The program was asked, through its lifecycle endpoint.
    Message,
    /// This signal was sent.
    Signal(i32),
}

/// Why a request could not be sent: an `errno`, or one of SlateOS's
/// sentences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// What the call itself said.
    Errno(i32),
    /// Close, to a program that does not answer requests.
    CannotClose,
    /// [`Via::Message`], to a program that does not answer requests.
    NoAnswer,
    /// A request to a process group, which only a signal reaches.
    Group,
    /// Close by [`Via::Signal`]: no signal means close.
    NoCloseSignal,
}

impl Refusal {
    /// The sentence for a refusal of SlateOS's own, as it follows the
    /// process it names; `None` for an `errno`, which the caller words as
    /// its C library does.
    #[must_use]
    pub const fn sentence(self) -> Option<&'static str> {
        match self {
            Self::Errno(_) => None,
            Self::CannotClose => {
                Some("answers no close request; a terminate stops it without asking")
            }
            Self::NoAnswer => Some("answers no requests"),
            Self::Group => Some("a process group is asked with signals only"),
            Self::NoCloseSignal => Some("no signal asks a program to close"),
        }
    }
}

/// What sending a request needs of the system.
pub trait Asker {
    /// `kill (pid, sig)`, `pid` as `kill(2)` takes it: an `errno` for a
    /// failure.
    fn kill(&self, pid: i32, sig: i32) -> Result<(), i32>;
    /// Ask process `pid` to close (`close`) or to terminate: an `errno` for
    /// a failure, `ENOSYS` when it does not answer requests -- it never
    /// registered, it closed its endpoint, or this is not SlateOS.
    fn ask(&self, pid: i32, close: bool) -> Result<(), i32>;
    /// Wait `ms` milliseconds.
    fn nap(&self, ms: u64);
}

/// The real thing: SlateOS's lifecycle calls and the C library's `kill`.
#[derive(Clone, Copy, Debug)]
pub struct System {
    /// Whether `kill` may reach a process group, or every process.
    groups: bool,
}

impl System {
    /// For a program that acts on processes it picked one at a time -- a
    /// task manager's selected row. Its `kill` refuses 0 and a negative PID
    /// with `EINVAL`, as `libcall::kill` does, so a PID read wrongly never
    /// reaches the program's own process group, or every process.
    pub const SINGLE: Self = Self { groups: false };
    /// For a program whose operands are process groups and broadcasts by
    /// design: the `kill` command (`kill -- -PGID`, `kill -9 -1`) and
    /// `killall -g`.
    pub const GROUPS: Self = Self { groups: true };
}

impl Asker for System {
    fn kill(&self, pid: i32, sig: i32) -> Result<(), i32> {
        if self.groups {
            libcall::kill_any(pid, sig)
        } else {
            libcall::kill(pid, sig)
        }
    }

    fn ask(&self, pid: i32, close: bool) -> Result<(), i32> {
        let p = u32::try_from(pid).map_err(|_| EINVAL)?;
        let kind = if close {
            lifecycle::Kind::Close
        } else {
            lifecycle::Kind::Terminate
        };
        lifecycle::ask(p, kind).map(drop).map_err(errno_of)
    }

    fn nap(&self, ms: u64) {
        // Woken early is a shorter wait, after which the process is looked
        // for again.
        let _ = libcall::clock::nanosleep(
            ms / 1000,
            u32::try_from(ms % 1000).map_or(0, |m| m.saturating_mul(1_000_000)),
        );
    }
}

/// A lifecycle call's failure in the words a signal's would have been;
/// `ENOSYS` for a program that does not answer.
fn errno_of(e: BusError) -> i32 {
    match e {
        BusError::NoSuchProcess => ESRCH,
        BusError::PermissionDenied => EPERM,
        BusError::QueueFull => EAGAIN,
        BusError::Unsupported => ENOSYS,
        BusError::ResourceExhausted => ENOMEM,
        BusError::InvalidArgument => EINVAL,
        // The kernel failed in a way that is not the caller's mistake.
        _ => EIO,
    }
}

/// Whether process (or group) `target` is still there: the null signal finds
/// it.
pub fn still_there<A: Asker + ?Sized>(sys: &A, target: i32) -> bool {
    sys.kill(target, 0) != Err(ESRCH)
}

/// Send `req` to `target` -- a PID, or a process group as `kill(2)` takes
/// one, which [`System::SINGLE`] refuses -- by the route `via` allows: a
/// program that answers is asked; one that does not is sent `SIGTERM` for
/// *terminate*, and refused for *close*, which no signal means; *force* is
/// `SIGKILL`.
///
/// # Errors
///
/// What stopped it: the call's `errno`, or one of the refusals.
pub fn deliver<A: Asker + ?Sized>(
    sys: &A,
    target: i32,
    req: Request,
    via: Via,
) -> Result<Route, Refusal> {
    deliver_with(sys, target, req, via, |sig| sys.kill(target, sig))
}

/// [`deliver`], every signal to `target` -- the null signal that looks for
/// it included -- sent by `signal`: `killall`'s, which signals a process
/// through the pidfd it opened, as upstream does where there are pidfds.
///
/// # Errors
///
/// As [`deliver`].
pub fn deliver_with<A: Asker + ?Sized>(
    sys: &A,
    target: i32,
    req: Request,
    via: Via,
    signal: impl Fn(i32) -> Result<(), i32>,
) -> Result<Route, Refusal> {
    let send = |sig: i32| {
        signal(sig)
            .map(|()| Route::Signal(sig))
            .map_err(Refusal::Errno)
    };
    let close = match req {
        Request::Force => return send(SIGKILL),
        Request::Close => true,
        Request::Terminate => false,
    };
    // A process group is reached by signals alone.
    if target <= 0 {
        return match (via, close) {
            (Via::Signal, true) => Err(Refusal::NoCloseSignal),
            (Via::Message, _) | (Via::Either, true) => Err(Refusal::Group),
            (_, false) => send(SIGTERM),
        };
    }
    // Asked first, rather than first asked whether it answers: a program
    // that closes its endpoint between the two is then sent the signal, not
    // an error.
    if via != Via::Signal {
        match sys.ask(target, close) {
            Ok(()) => return Ok(Route::Message),
            Err(ENOSYS) => {}
            Err(e) => return Err(Refusal::Errno(e)),
        }
    }
    // Not asked: say so -- unless the process is not there at all, which is
    // what to say then. (Off SlateOS asking says `ENOSYS` of a process that
    // does not exist too.)
    let absent = |refusal: Refusal| match signal(0) {
        Err(e) => Refusal::Errno(e),
        Ok(()) => refusal,
    };
    match (via, close) {
        (Via::Message, _) => Err(absent(Refusal::NoAnswer)),
        (Via::Signal, true) => Err(Refusal::NoCloseSignal),
        (Via::Either, true) => Err(absent(Refusal::CannotClose)),
        (_, false) => send(SIGTERM),
    }
}

/// `kill --timeout`'s escalation. `reached` holds the processes to follow --
/// each `(key, target)`, the key what the caller names it by -- which is
/// every one the first request was meant for that is there, whether or not
/// that request could be sent: `--close --timeout 10000 terminate` must
/// still terminate a program that answers no close request. For each `(ms,
/// request)` in turn: wait up to `ms` for them to end, then send `request` to
/// those still there. `refused` hears of each that could not be sent it; it
/// is still followed, and only its being gone ends that.
///
/// A process is found again by its PID. On SlateOS that is the process
/// asked, since the kernel never hands a PID out twice; on Linux, where it
/// does, a PID freed and reused while the escalation waits would be sent the
/// next request -- the window util-linux's `kill --timeout` closes with
/// pidfds, which SlateOS's own programs have no need of.
pub fn escalate<A: Asker + ?Sized, K>(
    sys: &A,
    mut reached: Vec<(K, i32)>,
    steps: &[(u64, Request)],
    via: Via,
    mut refused: impl FnMut(&K, Refusal),
) {
    for &(ms, next) in steps {
        let mut waited = 0u64;
        while waited < ms && reached.iter().any(|&(_, t)| still_there(sys, t)) {
            let step = TIMEOUT_POLL_MS.min(ms.saturating_sub(waited));
            sys.nap(step);
            waited = waited.saturating_add(step);
        }
        reached.retain(|&(_, t)| still_there(sys, t));
        let mut next_reached = Vec::with_capacity(reached.len());
        for (key, target) in reached {
            match deliver(sys, target, next, via) {
                Ok(_) => next_reached.push((key, target)),
                Err(Refusal::Errno(ESRCH)) => {}
                Err(why) => {
                    refused(&key, why);
                    next_reached.push((key, target));
                }
            }
        }
        reached = next_reached;
    }
}

/// A system for tests -- this crate's and the programs' that use it -- that
/// records what it was asked to do and does nothing.
#[cfg(any(test, feature = "fake"))]
pub mod fake {
    use super::{Asker, EINVAL, ENOSYS, ESRCH};
    use std::cell::RefCell;

    /// What was done: `("kill", pid, sig)`, `("close" | "terminate", pid,
    /// 0)` for a request a process answered, `("nap", 0, ms)`.
    pub type Did = (&'static str, i32, i32);

    /// The system. Every process is there and signals do nothing, except as
    /// the fields say.
    #[derive(Default)]
    pub struct Fake {
        /// Everything done, in order.
        pub did: RefCell<Vec<Did>>,
        /// The processes that answer requests; asking any other is `ENOSYS`.
        pub answering: Vec<i32>,
        /// The processes that end once a nap has passed.
        pub slow: Vec<i32>,
        /// The processes that are not there.
        pub gone: RefCell<Vec<i32>>,
        /// What asking a process that answers fails with, if it does.
        pub ask_fails: Option<i32>,
    }

    impl Asker for Fake {
        fn kill(&self, pid: i32, sig: i32) -> Result<(), i32> {
            self.did.borrow_mut().push(("kill", pid, sig));
            // As kill(2): a number that is no signal first, then the process.
            if !(0..=64).contains(&sig) {
                Err(EINVAL)
            } else if self.gone.borrow().contains(&pid) {
                Err(ESRCH)
            } else {
                Ok(())
            }
        }
        fn ask(&self, pid: i32, close: bool) -> Result<(), i32> {
            if self.gone.borrow().contains(&pid) {
                return Err(ESRCH);
            }
            if !self.answering.contains(&pid) {
                return Err(ENOSYS);
            }
            let what = if close { "close" } else { "terminate" };
            self.did.borrow_mut().push((what, pid, 0));
            self.ask_fails.map_or(Ok(()), Err)
        }
        fn nap(&self, ms: u64) {
            self.did
                .borrow_mut()
                .push(("nap", 0, i32::try_from(ms).unwrap_or(i32::MAX)));
            self.gone.borrow_mut().extend(self.slow.iter().copied());
        }
    }

    /// A system on which `answering` answer requests and `gone` are not
    /// there.
    #[must_use]
    pub fn fake(answering: &[i32], gone: &[i32]) -> Fake {
        Fake {
            answering: answering.to_vec(),
            gone: RefCell::new(gone.to_vec()),
            ..Fake::default()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::fake::{Fake, fake};
    use super::*;

    #[test]
    fn words_name_requests_and_routes() {
        assert_eq!(Request::named(b"close"), Some(Request::Close));
        assert_eq!(Request::named(b"terminate"), Some(Request::Terminate));
        assert_eq!(Request::named(b"force"), Some(Request::Force));
        assert_eq!(Request::named(b"Force"), None);
        assert_eq!(Via::named(b"message"), Some(Via::Message));
        assert_eq!(Via::named(b"signal"), Some(Via::Signal));
        assert_eq!(Via::named(b"pigeon"), None);
        assert_eq!(Via::default(), Via::Either);
    }

    #[test]
    fn every_refusal_but_an_errno_has_a_sentence() {
        assert_eq!(Refusal::Errno(ESRCH).sentence(), None);
        for r in [
            Refusal::CannotClose,
            Refusal::NoAnswer,
            Refusal::Group,
            Refusal::NoCloseSignal,
        ] {
            assert!(r.sentence().is_some_and(|s| !s.is_empty()), "{r:?}");
        }
    }

    #[test]
    fn terminate_is_the_message_where_answered_and_sigterm_elsewhere() {
        let sys = fake(&[5], &[]);
        assert_eq!(
            deliver(&sys, 5, Request::Terminate, Via::Either),
            Ok(Route::Message)
        );
        assert_eq!(
            deliver(&sys, 6, Request::Terminate, Via::Either),
            Ok(Route::Signal(SIGTERM))
        );
        assert_eq!(
            *sys.did.borrow(),
            vec![("terminate", 5, 0), ("kill", 6, SIGTERM)]
        );
    }

    #[test]
    fn close_is_refused_where_nothing_answers_unless_nothing_is_there() {
        let sys = fake(&[5], &[7]);
        assert_eq!(
            deliver(&sys, 5, Request::Close, Via::Either),
            Ok(Route::Message)
        );
        assert_eq!(
            deliver(&sys, 6, Request::Close, Via::Either),
            Err(Refusal::CannotClose)
        );
        // Asking says it is gone; nothing is signalled.
        assert_eq!(
            deliver(&sys, 7, Request::Close, Via::Either),
            Err(Refusal::Errno(ESRCH))
        );
        assert_eq!(
            deliver(&sys, 5, Request::Close, Via::Signal),
            Err(Refusal::NoCloseSignal)
        );
        assert_eq!(*sys.did.borrow(), vec![("close", 5, 0), ("kill", 6, 0)]);
    }

    #[test]
    fn a_process_that_does_not_answer_and_is_not_there_is_said_to_be_gone() {
        // Off SlateOS asking says ENOSYS whether the process exists or not:
        // the null signal tells the two apart.
        struct NobodyAnswers(Fake);
        impl Asker for NobodyAnswers {
            fn kill(&self, pid: i32, sig: i32) -> Result<(), i32> {
                self.0.kill(pid, sig)
            }
            fn ask(&self, _pid: i32, _close: bool) -> Result<(), i32> {
                Err(ENOSYS)
            }
            fn nap(&self, ms: u64) {
                self.0.nap(ms);
            }
        }
        let sys = NobodyAnswers(fake(&[], &[7]));
        assert_eq!(
            deliver(&sys, 7, Request::Close, Via::Either),
            Err(Refusal::Errno(ESRCH))
        );
        assert_eq!(
            deliver(&sys, 7, Request::Terminate, Via::Message),
            Err(Refusal::Errno(ESRCH))
        );
    }

    #[test]
    fn a_route_insisted_on_is_the_only_one_tried() {
        let sys = fake(&[5], &[7]);
        assert_eq!(
            deliver(&sys, 5, Request::Terminate, Via::Signal),
            Ok(Route::Signal(SIGTERM))
        );
        assert_eq!(
            deliver(&sys, 6, Request::Terminate, Via::Message),
            Err(Refusal::NoAnswer)
        );
        assert_eq!(
            deliver(&sys, 7, Request::Terminate, Via::Message),
            Err(Refusal::Errno(ESRCH))
        );
        assert_eq!(
            *sys.did.borrow(),
            vec![("kill", 5, SIGTERM), ("kill", 6, 0)]
        );
    }

    #[test]
    fn force_is_sigkill_and_a_group_is_only_signalled() {
        let sys = fake(&[5], &[]);
        assert_eq!(
            deliver(&sys, 5, Request::Force, Via::Message),
            Ok(Route::Signal(SIGKILL))
        );
        assert_eq!(
            deliver(&sys, -5, Request::Terminate, Via::Either),
            Ok(Route::Signal(SIGTERM))
        );
        assert_eq!(
            deliver(&sys, 0, Request::Force, Via::Either),
            Ok(Route::Signal(SIGKILL))
        );
        for (req, via) in [
            (Request::Close, Via::Either),
            (Request::Terminate, Via::Message),
        ] {
            assert_eq!(deliver(&sys, -5, req, via), Err(Refusal::Group));
        }
        // Signals were what was asked for: it is the close they cannot say.
        assert_eq!(
            deliver(&sys, -5, Request::Close, Via::Signal),
            Err(Refusal::NoCloseSignal)
        );
        assert_eq!(
            *sys.did.borrow(),
            vec![
                ("kill", 5, SIGKILL),
                ("kill", -5, SIGTERM),
                ("kill", 0, SIGKILL)
            ]
        );
    }

    #[test]
    fn deliver_with_signals_through_its_own_path() {
        let sys = fake(&[], &[]);
        let sent = std::cell::RefCell::new(Vec::new());
        let signal = |sig| {
            sent.borrow_mut().push(sig);
            Ok(())
        };
        assert_eq!(
            deliver_with(&sys, 6, Request::Close, Via::Either, signal),
            Err(Refusal::CannotClose)
        );
        assert_eq!(
            deliver_with(&sys, 6, Request::Terminate, Via::Either, signal),
            Ok(Route::Signal(SIGTERM))
        );
        // The null signal that found it, then SIGTERM; nothing by `kill`.
        assert_eq!(*sent.borrow(), vec![0, SIGTERM]);
        assert!(sys.did.borrow().is_empty());
    }

    #[test]
    fn a_failed_ask_is_its_errno() {
        let sys = Fake {
            answering: vec![5],
            ask_fails: Some(EPERM),
            ..Fake::default()
        };
        assert_eq!(
            deliver(&sys, 5, Request::Close, Via::Either),
            Err(Refusal::Errno(EPERM))
        );
        assert_eq!(errno_of(BusError::NoSuchProcess), ESRCH);
        assert_eq!(errno_of(BusError::PermissionDenied), EPERM);
        assert_eq!(errno_of(BusError::QueueFull), EAGAIN);
        assert_eq!(errno_of(BusError::Unsupported), ENOSYS);
        assert_eq!(errno_of(BusError::ResourceExhausted), ENOMEM);
        assert_eq!(errno_of(BusError::InvalidArgument), EINVAL);
        assert_eq!(errno_of(BusError::Malformed), EIO);
        assert_eq!(errno_of(BusError::Unknown(-1)), EIO);
    }

    #[test]
    fn escalation_waits_then_sends_to_what_is_still_there() {
        // 5 ends after the first nap; 6 stays.
        let sys = Fake {
            answering: vec![5, 6],
            slow: vec![5],
            ..Fake::default()
        };
        let mut refused = Vec::new();
        escalate(
            &sys,
            vec![("five", 5), ("six", 6)],
            &[(30, Request::Force), (0, Request::Close)],
            Via::Either,
            |k, why| refused.push((*k, why)),
        );
        assert_eq!(
            *sys.did.borrow(),
            vec![
                ("kill", 5, 0),
                ("nap", 0, 20),
                ("kill", 5, 0),
                ("kill", 6, 0),
                ("nap", 0, 10),
                ("kill", 5, 0),
                ("kill", 6, 0),
                ("kill", 6, SIGKILL),
                // No wait at 0 ms; 6 is asked to close.
                ("kill", 6, 0),
                ("close", 6, 0),
            ]
        );
        assert!(refused.is_empty());
    }

    #[test]
    fn a_refused_step_does_not_end_the_escalation() {
        // `--close --timeout 0 terminate --timeout 0 force` to a program that
        // answers nothing: the close is refused, and the terminate and the
        // force are still sent.
        let sys = fake(&[], &[]);
        let mut refused = Vec::new();
        escalate(
            &sys,
            vec![(6, 6)],
            &[
                (0, Request::Close),
                (0, Request::Terminate),
                (0, Request::Force),
            ],
            Via::Either,
            |k, why| refused.push((*k, why)),
        );
        assert_eq!(refused, vec![(6, Refusal::CannotClose)]);
        let did = sys.did.borrow();
        assert!(did.contains(&("kill", 6, SIGTERM)), "{did:?}");
        assert!(did.contains(&("kill", 6, SIGKILL)), "{did:?}");
    }

    #[test]
    fn a_process_gone_before_its_step_is_neither_sent_nor_reported() {
        let sys = fake(&[], &[6]);
        let mut refused = Vec::new();
        escalate(
            &sys,
            vec![(6, 6)],
            &[(0, Request::Force)],
            Via::Either,
            |k, why| refused.push((*k, why)),
        );
        assert!(refused.is_empty());
        assert_eq!(*sys.did.borrow(), vec![("kill", 6, 0)]);
    }

    #[test]
    fn the_single_process_system_refuses_a_group_and_everyone() {
        assert_eq!(System::SINGLE.kill(0, 0), Err(EINVAL));
        assert_eq!(System::SINGLE.kill(-1, 0), Err(EINVAL));
        assert_eq!(System::SINGLE.kill(-1234, 0), Err(EINVAL));
        // The kill command's reaches its own group (the null signal sends
        // nothing): wherever it can signal at all, it is not refused.
        assert_ne!(System::GROUPS.kill(0, 0), Err(EINVAL));
        // A group refused without a word to its members.
        assert_eq!(
            deliver(&System::SINGLE, -1, Request::Force, Via::Either),
            Err(Refusal::Errno(EINVAL))
        );
    }

    /// Off SlateOS nothing answers: a terminate is `SIGTERM`.
    #[cfg(not(target_vendor = "slateos"))]
    #[test]
    fn off_slateos_nothing_answers() {
        assert_eq!(System::SINGLE.ask(1, false), Err(ENOSYS));
        assert_eq!(System::SINGLE.ask(-1, false), Err(EINVAL));
    }
}
