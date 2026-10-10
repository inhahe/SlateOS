//! The credential service's loop: each program's connection accepted and
//! answered, one at a time, and the vault locked once it has sat unused as
//! long as it may.
//!
//! [`run`] is the whole of it, over a [`Listener`] -- on SlateOS the service
//! registry's (`Registered`, built for SlateOS only) -- so a test drives it
//! with no kernel.
//!
//! # One program at a time
//!
//! A connection is answered to its end before the next is taken: the user is
//! asked about one program at a time, never shown two prompts at once, both
//! wanting the master password. A program that connects and says nothing
//! holds the service for [`QUERY_PATIENCE`](credentials::service::QUERY_PATIENCE)
//! at most.
//!
//! # A program cannot stop the service
//!
//! Whatever goes wrong with one connection -- a program that hangs up, says
//! something that is not a query, or never asks -- is logged, and the
//! service goes on to the next. Only the listener failing stops it, and the
//! log failing: a service that gives out passwords and cannot say so stops
//! rather than giving them out unrecorded.
//!
//! # What is logged
//!
//! One JSON-lines record per connection, in `journalrec`'s spelling, which
//! `journalctl` reads: which program -- its path and process -- for which
//! domain, and how it ended. Never a password, and never whose login was
//! given. A password given is logged at `notice`; a program with no key
//! asking, at `warning`.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use credentials::service::{Peer, Prompt, Served, Service, Vault, Why, serve};
use guiremote::client::Transport;
use journalrec::Record;

/// The name the service's records carry.
const LOG_NAME: &str = "credentials";

/// Where the service's connections come from.
pub trait Listener {
    /// One program's connection.
    type Conn: Transport<Error = io::Error> + Peer;

    /// The next connection waiting, or `None` when none is.
    ///
    /// # Errors
    ///
    /// The listener has failed, and the service can take no more.
    fn accept(&mut self) -> io::Result<Option<Self::Conn>>;

    /// Wait until a connection may be waiting, or `timeout` has passed --
    /// for as long as it takes, given `None`. May return early, with
    /// nothing waiting.
    ///
    /// # Errors
    ///
    /// As [`accept`](Self::accept).
    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<()>;
}

/// On SlateOS, the service registry: the name
/// [`SERVICE`](credentials::SERVICE) registered, and its connections
/// accepted as they come.
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
pub struct Registered {
    listener: guiremote::channel::ChannelListener,
}

#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
impl Registered {
    /// Register the service.
    ///
    /// # Errors
    ///
    /// The kernel's: another process has the name, or this one lacks the
    /// capability to register it.
    pub fn register() -> io::Result<Self> {
        Ok(Self {
            listener: guiremote::channel::ChannelListener::register(credentials::SERVICE)?,
        })
    }
}

#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
impl Listener for Registered {
    type Conn = guiremote::channel::ChannelConn;

    fn accept(&mut self) -> io::Result<Option<Self::Conn>> {
        self.listener.accept()
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let mut waits = guiremote::wait::WaitSet::new();
        waits.add_source(&self.listener);
        waits.wait(timeout).map(|_| ())
    }
}

/// Run the service until `stop` is set: each connection `listener` takes
/// answered by `service`, and a record of each written to `log`.
///
/// Between connections it waits for the next, and wakes in time to lock the
/// vault when it has sat unused as long as it may
/// ([`Service::until_expiry`]).
///
/// # Errors
///
/// The listener's, and the log's -- see the module doc.
pub fn run<L, V, P>(
    listener: &mut L,
    service: &mut Service<V, P>,
    stop: &AtomicBool,
    log: &mut dyn Write,
) -> io::Result<()>
where
    L: Listener,
    V: Vault,
    P: Prompt,
{
    while !stop.load(Ordering::Acquire) {
        service.expire();
        while let Some(conn) = listener.accept()? {
            let line = match serve(service, conn) {
                Ok(served) => record(&served, now()),
                Err(e) => dropped(&e, now()),
            };
            writeln!(log, "{line}")?;
            log.flush()?;
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        listener.wait(service.until_expiry())?;
    }
    Ok(())
}

/// Seconds since the epoch, for a record's time.
fn now() -> u64 {
    // A clock set before 1970 records as the epoch: the record still says
    // what happened, and its time is as wrong as the clock it was read from.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// How an ask ended, for the log: its level, one word for a machine and
/// some for a person.
const fn told(why: Why) -> (&'static str, &'static str, &'static str) {
    match why {
        Why::Allowed => ("notice", "allowed", "given: the user allowed it"),
        Why::AllowedEarlier => (
            "notice",
            "allowed-earlier",
            "given: the user allowed it until the vault locks",
        ),
        Why::Refused => ("info", "refused", "refused by the user"),
        Why::Quiet => (
            "info",
            "quiet",
            "refused unasked: the user refused it a moment ago",
        ),
        Why::CouldNotAsk => (
            "info",
            "could-not-ask",
            "refused: the user could not be asked",
        ),
        Why::WrongPassword => (
            "info",
            "wrong-password",
            "refused: the master password was wrong at every try",
        ),
        Why::NoLogin => ("info", "no-login", "refused: no saved login is for it"),
        Why::NoKey => ("warning", "no-key", "refused: it holds no key to ask"),
        Why::Unknown => ("warning", "unknown", "refused: the kernel named no program"),
        Why::Failed(_) => (
            "err",
            "failed",
            "refused: the kernel or the vault could not be read",
        ),
        Why::TooLong => ("err", "too-long", "refused: the login is too long to send"),
    }
}

/// The record of a connection answered.
fn record(served: &Served, ts: u64) -> String {
    let (level, outcome, words) = told(served.why);
    let domain = match credentials::matching::domain(&served.target) {
        "" => served.target.as_str(),
        domain => domain,
    };
    let program = served
        .program
        .as_ref()
        .map(|program| pathcodec::display_path(&program.exe));
    let who = program
        .as_deref()
        .unwrap_or("a program the kernel could not name");
    let mut extra = vec![
        ("domain".to_owned(), domain.to_owned()),
        ("outcome".to_owned(), outcome.to_owned()),
    ];
    if let Why::Failed(kind) = served.why {
        extra.push(("error".to_owned(), kind.to_string()));
    }
    if let Some(program) = &program {
        extra.push(("program".to_owned(), program.clone()));
    }
    Record {
        ts,
        level: level.to_owned(),
        service: LOG_NAME.to_owned(),
        msg: format!("{who} asked for a password for {domain}: {words}"),
        pid: served.program.as_ref().map(|program| program.pid),
    }
    .to_json_line_with(&extra)
}

/// The record of a connection given up on before it was answered.
fn dropped(error: &io::Error, ts: u64) -> String {
    Record {
        ts,
        level: "warning".to_owned(),
        service: LOG_NAME.to_owned(),
        msg: format!("a connection was given up on unanswered: {error}"),
        pid: None,
    }
    .to_json_line_with(&[("outcome".to_owned(), "dropped".to_owned())])
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
