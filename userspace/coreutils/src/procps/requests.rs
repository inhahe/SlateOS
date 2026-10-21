//! The command-line half of SlateOS's three ways to stop a process --
//! *close*, *terminate*, *force* (design-decisions §1072, the operator's
//! answer to B-Q22) -- for `kill`, `pkill` and `killall`, which take the same
//! options. How a request is routed is [`procstop`]'s, re-exported here.
//!
//! | option | asks |
//! |---|---|
//! | `--terminate` (the default) | terminate: the message if the program answers it, else `SIGTERM` |
//! | `--close` | close: the message if it answers; refused otherwise |
//! | `--force` | `SIGKILL` |
//! | `--via=message`, `--via=signal` | only that route: a program that does not answer is refused, or no message is tried |
//! | `--timeout MS REQUEST` | `REQUEST` to each process still there `MS` milliseconds later; repeatable |
//!
//! A signal the user names -- `-9`, `-s INT`, `-q 5` -- is sent as named, the
//! Linux way, so a request or a route beside one is a contradiction; a
//! `--timeout` after one is not.
//!
//! The options are matched only when spelled in full ([`NAMES`], for the
//! parser's `whole_only`), so every abbreviation means what it does upstream
//! -- `pkill --term` is `--terminal`. Off SlateOS no program answers, so a
//! terminate is always `SIGTERM`: the programs behave there as upstream's.

pub use procstop::{
    Asker, ESRCH, Refusal, Request, Route, SIGKILL, SIGTERM, System, Via, deliver, deliver_with,
    escalate, still_there,
};

use super::strutils;
use crate::getopt::Takes;

/// The options, for the end of a program's long-option table.
pub const LONGS: &[(&str, Takes)] = &[
    ("close", Takes::Nothing),
    ("terminate", Takes::Nothing),
    ("force", Takes::Nothing),
    ("via", Takes::Required),
    ("timeout", Takes::Required),
];

/// Their names, which the parser is to match only in full.
pub const NAMES: &[&str] = &["close", "terminate", "force", "via", "timeout"];

/// The paragraph under each program's usage. Its first line is what the
/// differential harnesses find it by, to take it out before comparing with
/// upstream's usage, which has no counterpart; every line after it is
/// indented, which is how they know where it ends.
pub const USAGE: &[u8] =
    b"\nSlateOS's requests, which a program may answer itself (instead of a signal):\n \
    --close                ask each process to close, as its close command does\n \
    --terminate            ask each process to stop now, asking nothing (the default)\n \
    --force                end each process at once (SIGKILL)\n \
    --via=message|signal   send a request only as a message, or only as a signal\n \
    --timeout <ms> <request>\n                        \
    if a process is still there <ms> later, send it <request>\n";

/// What the command line asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--close`, `--terminate` or `--force`, the last given.
    pub requested: Option<Request>,
    /// `--via`.
    pub via: Via,
    /// Each `--timeout`: milliseconds, and what to send then.
    pub escalations: Vec<(u64, Request)>,
}

impl Options {
    /// The complaint when they are given with a named signal.
    pub const CONFLICT: &'static str =
        "a signal and a request (--close, --terminate, --force, --via) cannot both be given";

    /// Take long option `name`, with its `value`, if it is one of these:
    /// `Ok(true)` if it was, `Ok(false)` if it is the program's own. `next`
    /// takes the word after it, with which `--timeout` names its request.
    ///
    /// # Errors
    ///
    /// The complaint, for a `--via` or a `--timeout` that is wrong.
    pub fn take(
        &mut self,
        name: &str,
        value: Option<&[u8]>,
        next: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Result<bool, &'static str> {
        match name {
            "close" => self.requested = Some(Request::Close),
            "terminate" => self.requested = Some(Request::Terminate),
            "force" => self.requested = Some(Request::Force),
            "via" => {
                self.via = value
                    .and_then(Via::named)
                    .ok_or("--via takes message or signal")?;
            }
            "timeout" => {
                let ms = value
                    .and_then(|v| strutils::strtol(v).ok())
                    .and_then(|v| u64::try_from(v).ok());
                let then = next().and_then(|w| Request::named(&w));
                let (Some(ms), Some(then)) = (ms, then) else {
                    return Err(
                        "--timeout takes milliseconds and then a request: close, terminate or force",
                    );
                };
                self.escalations.push((ms, then));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Whether a request or a route was named, which a named signal cannot
    /// be sent beside.
    #[must_use]
    pub fn conflict_with_a_signal(&self) -> bool {
        self.requested.is_some() || self.via != Via::Either
    }

    /// What each process is sent first: the request named, else
    /// *terminate*.
    #[must_use]
    pub fn request(&self) -> Request {
        self.requested.unwrap_or(Request::Terminate)
    }
}

/// A refusal's reason, as a program prints it after the process it names:
/// the C library's words for an `errno`.
pub trait Explain {
    /// The reason.
    fn text(self) -> String;
}

impl Explain for Refusal {
    fn text(self) -> String {
        match (self, self.sentence()) {
            (_, Some(sentence)) => sentence.into(),
            (Refusal::Errno(e), None) => {
                crate::errmsg::strerror(&std::io::Error::from_raw_os_error(e))
            }
            // Every refusal but an errno has a sentence.
            (_, None) => String::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_agree() {
        let longs: Vec<&str> = LONGS.iter().map(|&(n, _)| n).collect();
        assert_eq!(longs, NAMES);
    }

    #[test]
    fn the_usage_paragraph_is_found_by_its_first_line() {
        assert!(USAGE.starts_with(b"\nSlateOS's requests, which a program may answer itself"));
        let text = std::str::from_utf8(USAGE).unwrap();
        assert!(text.lines().skip(2).all(|l| l.starts_with(' ')));
    }

    #[test]
    fn options_take_their_own_and_leave_the_rest() {
        let mut o = Options::default();
        assert_eq!(o.take("close", None, || None), Ok(true));
        assert_eq!(o.take("force", None, || None), Ok(true));
        assert_eq!(o.request(), Request::Force);
        assert_eq!(o.take("terminate", None, || None), Ok(true));
        assert_eq!(o.request(), Request::Terminate);
        assert_eq!(o.take("via", Some(b"signal"), || None), Ok(true));
        assert_eq!(o.via, Via::Signal);
        assert!(o.take("via", Some(b"post"), || None).is_err());
        assert!(o.take("via", None, || None).is_err());
        assert_eq!(
            o.take("timeout", Some(b"500"), || Some(b"force".to_vec())),
            Ok(true)
        );
        assert_eq!(o.escalations, vec![(500, Request::Force)]);
        for (ms, then) in [
            (Some(&b"soon"[..]), Some(&b"force"[..])),
            (Some(b"-1"), Some(b"force")),
            (Some(b"5"), Some(b"harder")),
            (Some(b"5"), None),
            (None, Some(b"force")),
        ] {
            let next = || then.map(<[u8]>::to_vec);
            assert!(o.take("timeout", ms, next).is_err(), "{ms:?} {then:?}");
        }
        assert_eq!(o.escalations.len(), 1);
        assert_eq!(o.take("signal", Some(b"9"), || None), Ok(false));
    }

    #[test]
    fn only_a_request_or_a_route_conflicts_with_a_signal() {
        let mut o = Options::default();
        assert!(!o.conflict_with_a_signal());
        assert_eq!(o.request(), Request::Terminate);
        o.escalations.push((5, Request::Force));
        assert!(!o.conflict_with_a_signal());
        o.via = Via::Message;
        assert!(o.conflict_with_a_signal());
        let o = Options {
            requested: Some(Request::Terminate),
            ..Options::default()
        };
        assert!(o.conflict_with_a_signal());
    }

    #[test]
    fn a_refusal_is_explained_in_the_c_library_s_words_or_slateos_s() {
        let esrch = std::io::Error::from_raw_os_error(ESRCH);
        assert_eq!(
            Refusal::Errno(ESRCH).text(),
            crate::errmsg::strerror(&esrch)
        );
        assert_eq!(Refusal::NoAnswer.text(), "answers no requests");
    }
}
