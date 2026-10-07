//! The credential service's judgement: whether a program asking for a
//! password gets one, and which.
//!
//! `design-decisions.md` §1417 (the operator's): a program may ask the
//! password manager for a password only if it holds a key for it -- a
//! capability the kernel checks, not a list the service keeps -- and only
//! with the user's say-so, given in a prompt that shows which program is
//! asking. [`Service::answer`] is that rule, written against three things it
//! is handed rather than three it reaches for, so that each can be stood in
//! for by a test:
//!
//! - a [`Peer`]: the connection's other end as the kernel recorded it --
//!   which program it is, and whether it holds the key;
//! - a [`Vault`]: the user's saved logins, locked or open;
//! - a [`Prompt`]: the user, asked.
//!
//! # What a program gets
//!
//! [`Answer::Refused`] unless, in order:
//!
//! 1. **The kernel names it**: the executable of the process that connected.
//! 2. **It holds the key.** Asked *after* the name is read, on purpose: the
//!    kernel answers yes only while the process that connected still holds
//!    its end, so the name read is that process's -- not that of one that
//!    took its pid after it exited.
//! 3. **The user says yes.** A locked vault is opened only with the master
//!    password typed into that same prompt, [`TRIES`] tries at most. Not
//!    asked, when the vault is open, for a login the user already let this
//!    program have until it locks. And not asked -- refused unasked -- for
//!    [`QUIET`] after the user refused the program, so it cannot hold the
//!    user under a stream of prompts; that spell stops prompts only, and
//!    takes back nothing the user allowed until the vault locks.
//! 4. **The vault holds a login for what it asked**: the best match
//!    ([`matching`](crate::matching)) for its target and, when it names one,
//!    its user name. Where several match equally, the user picks. With the
//!    vault open, a target it holds nothing for is refused without asking --
//!    there is nothing to ask about.
//!
//! The program is told only the answer -- a refusal says nothing about why.
//! [`Outcome::why`] says, for the service's log and for tests.
//!
//! # "Allow until locked", not "always allow"
//!
//! The prompt offers to allow a program a login once, or until the vault
//! locks -- not for good. A choice that outlived the vault's lock would be
//! kept in a file, and every file the service could keep is one the user's
//! programs can write: a program holding the key could write itself an
//! "always" and never be asked again, which is the say-so §1417 requires
//! taken away by the program it protects against. And it would buy almost
//! nothing: once the vault locks, the next ask needs the master password
//! typed into the prompt anyway. So the choice lives in this process's
//! memory, and goes when the vault locks or the service stops.
//!
//! # Locking
//!
//! The vault stays open for as long as its own auto-lock setting says it may
//! sit unused ([`Vault::stays_open`]) -- the password manager's setting,
//! which the user chose for exactly this. [`Service::expire`] locks it once
//! that has passed; the daemon calls it when [`Service::next_expiry`] comes.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use svcconn::Waiting;

use crate::matching::{MatchPriority, match_url};
use crate::protocol::{self, Answer, Query, Secret};

/// How long a program the user refused is refused unasked.
pub const QUIET: Duration = Duration::from_mins(1);

/// How many master passwords one ask may try.
pub const TRIES: usize = 3;

/// How long [`serve`] waits for a program's query on a fresh connection: a
/// program sends one as soon as it connects, so this is generous for any
/// that means to, and short enough that one that never does cannot hold the
/// service.
pub const QUERY_PATIENCE: Duration = Duration::from_secs(5);

/// A program, as the kernel names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The process that asked.
    pub pid: u32,
    /// The executable it runs: what the prompt shows, and what the user's
    /// "until locked" is for.
    pub exe: PathBuf,
}

/// The other end of a connection, as the kernel recorded it when the
/// connection was made -- never as the program describes itself.
pub trait Peer {
    /// The program at the other end; `None` when the kernel recorded none,
    /// or its executable cannot be named -- a process that has exited.
    ///
    /// # Errors
    ///
    /// The kernel's, other than those.
    fn program(&self) -> io::Result<Option<Program>>;

    /// Whether the program at the other end holds the key to ask, and still
    /// holds its end of the connection; `false` when the kernel cannot say.
    ///
    /// # Errors
    ///
    /// The kernel's, other than that.
    fn holds_key(&self) -> io::Result<bool>;
}

/// On SlateOS, a channel accepted from the service registry: the kernel's
/// record of who connected (`peer_cred`), the program that process runs
/// (`/proc/<pid>/exe`), and lane A's key check (`peer_has_key`, §1518).
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
impl Peer for guiremote::channel::ChannelConn {
    fn program(&self) -> io::Result<Option<Program>> {
        let Some(cred) = self.peer_cred()? else {
            return Ok(None);
        };
        match std::fs::read_link(format!("/proc/{}/exe", cred.pid)) {
            Ok(exe) => Ok(Some(Program { pid: cred.pid, exe })),
            // Gone, or never ran a program: nothing to name.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn holds_key(&self) -> io::Result<bool> {
        Ok(self.peer_has_key()? == Some(true))
    }
}

/// One login the vault holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLogin {
    /// The vault's own name for it, which outlives an edit of everything
    /// else here.
    pub id: u64,
    /// The address it is for; empty where it has none.
    pub url: String,
    /// What the user calls it -- often the site's domain.
    pub site: String,
    /// Its user name.
    pub username: String,
    /// Its password.
    pub password: Secret,
}

impl SavedLogin {
    /// What it is for: its address, or where it has none, its name.
    #[must_use]
    pub fn target(&self) -> &str {
        if self.url.is_empty() {
            &self.site
        } else {
            &self.url
        }
    }

    fn answer(&self) -> Answer {
        Answer::Login {
            username: self.username.clone(),
            password: self.password.clone(),
        }
    }
}

/// The user's saved logins.
pub trait Vault {
    /// Whether it is locked: its logins unread until [`unlock`](Self::unlock).
    fn is_locked(&self) -> bool;

    /// Open it with `master`, the password the user typed: `false` if that is
    /// not its password.
    ///
    /// # Errors
    ///
    /// It cannot be read.
    fn unlock(&mut self, master: &Secret) -> io::Result<bool>;

    /// Lock it, forgetting its logins and its key.
    fn lock(&mut self);

    /// Its logins: none while it is locked.
    fn logins(&self) -> &[SavedLogin];

    /// How long it may stay open unused: the user's own auto-lock setting.
    fn stays_open(&self) -> Duration;
}

/// What the user is shown when asked.
#[derive(Clone, Copy, Debug)]
pub struct Asking<'a> {
    /// The program asking.
    pub program: &'a Program,
    /// What it asks a login for, as it asked.
    pub target: &'a str,
    /// Whose login, when it named a user.
    pub username: Option<&'a str>,
    /// Whether the vault is locked -- so allowing takes its master password.
    pub locked: bool,
    /// Whether the master password last typed was not the vault's.
    pub wrong: bool,
}

/// One of several logins matching equally, as the user is shown it: never
/// its password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice<'a> {
    /// What it is for.
    pub target: &'a str,
    /// Its user name.
    pub username: &'a str,
}

/// How long the user allowed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// This once.
    Once,
    /// Until the vault locks.
    UntilLocked,
}

/// What the user said.
#[derive(Debug)]
pub enum Said {
    /// No.
    Refuse,
    /// Yes -- with the master password, when the vault was locked.
    Allow {
        /// For how long.
        scope: Scope,
        /// The master password typed; read only when the vault is locked.
        master: Option<Secret>,
    },
    /// Nothing: the user could not be asked -- no display to show the
    /// prompt on, or it went before they answered. A refusal, but not the
    /// user's, so the program is not quieted for it.
    CouldNotAsk,
}

/// Which login the user picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    /// The choice at this index.
    Login(usize),
    /// None of them.
    Nothing,
    /// Nothing: the user could not be asked, as for [`Said::CouldNotAsk`].
    CouldNotAsk,
}

/// The user, asked.
pub trait Prompt {
    /// Whether the program may have a login for what it asked.
    fn ask(&mut self, asking: &Asking<'_>) -> Said;

    /// Which of `choices` -- logins matching equally -- the program may
    /// have.
    fn choose(&mut self, asking: &Asking<'_>, choices: &[Choice<'_>]) -> Picked;
}

/// Why an ask ended as it did: for the service's log and for tests, never
/// for the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The kernel named no program at the other end.
    Unknown,
    /// The program holds no key to ask.
    NoKey,
    /// The user refused this program a moment ago.
    Quiet,
    /// The user refused.
    Refused,
    /// The user could not be asked.
    CouldNotAsk,
    /// The master password was wrong [`TRIES`] times.
    WrongPassword,
    /// The vault holds no login for what was asked.
    NoLogin,
    /// The kernel could not be asked, or the vault read.
    Failed(io::ErrorKind),
    /// The login is longer than the protocol carries.
    TooLong,
    /// The user allowed it.
    Allowed,
    /// The user allowed this program this login until the vault locks.
    AllowedEarlier,
}

/// What an ask came to.
#[derive(Debug)]
pub struct Outcome {
    /// What the program is told.
    pub answer: Answer,
    /// Why.
    pub why: Why,
    /// Who asked, as the kernel named it -- a program refused for holding no
    /// key included; `None` when the kernel named nobody. For the log.
    pub program: Option<Program>,
}

impl Outcome {
    const fn refused(why: Why) -> Self {
        Self {
            answer: Answer::Refused,
            why,
            program: None,
        }
    }

    const fn given(answer: Answer, why: Why) -> Self {
        Self {
            answer,
            why,
            program: None,
        }
    }
}

/// The credential service: the vault, the prompt, and what the user said.
pub struct Service<V, P> {
    vault: V,
    prompt: P,
    /// What the user allowed each program until the vault locks: the ids of
    /// its logins. Empty whenever the vault is locked.
    allowed: HashMap<PathBuf, HashSet<u64>>,
    /// Programs the user refused, and until when they are refused unasked.
    quiet: HashMap<PathBuf, Instant>,
    /// When the vault was last used, while this service holds it open.
    last_used: Option<Instant>,
    clock: Box<dyn Fn() -> Instant + Send>,
}

impl<V, P> fmt::Debug for Service<V, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("allowed", &self.allowed)
            .field("quiet", &self.quiet)
            .field("last_used", &self.last_used)
            .finish_non_exhaustive()
    }
}

impl<V: Vault, P: Prompt> Service<V, P> {
    /// The service over `vault`, asking through `prompt`.
    #[must_use]
    pub fn new(vault: V, prompt: P) -> Self {
        Self {
            vault,
            prompt,
            allowed: HashMap::new(),
            quiet: HashMap::new(),
            last_used: None,
            clock: Box::new(Instant::now),
        }
    }

    /// The same, telling the time by `clock`: for tests.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Fn() -> Instant + Send + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Answer `query`, asked by `peer`.
    pub fn answer(&mut self, peer: &impl Peer, query: &Query) -> Outcome {
        self.expire();
        match identify(peer) {
            Ok(program) => {
                let mut outcome = self.answer_program(&program, query);
                outcome.program = Some(program);
                outcome
            }
            Err((why, program)) => Outcome {
                program,
                ..Outcome::refused(why)
            },
        }
    }

    /// Answer `query`, asked by `program`, which may ask.
    fn answer_program(&mut self, program: &Program, query: &Query) -> Outcome {
        let now = (self.clock)();
        if self.vault.is_locked() {
            // Whatever the user allowed went with the vault's last lock.
            self.allowed.clear();
        } else {
            let found = candidates(self.vault.logins(), query);
            if found.is_empty() {
                return Outcome::refused(Why::NoLogin);
            }
            let allowed = self.allowed.get(&program.exe);
            let mut earlier = found
                .iter()
                .filter(|login| allowed.is_some_and(|ids| ids.contains(&login.id)));
            if let (Some(login), None) = (earlier.next(), earlier.next()) {
                let answer = login.answer();
                self.last_used = Some(now);
                return Outcome::given(answer, Why::AllowedEarlier);
            }
        }
        if self
            .quiet
            .get(&program.exe)
            .is_some_and(|until| now < *until)
        {
            return Outcome::refused(Why::Quiet);
        }
        self.ask(program, query)
    }

    /// Ask the user about `program`'s `query`.
    fn ask(&mut self, program: &Program, query: &Query) -> Outcome {
        let mut wrong = false;
        for _ in 0..TRIES {
            let locked = self.vault.is_locked();
            let asking = Asking {
                program,
                target: &query.target,
                username: query.username.as_deref(),
                locked,
                wrong,
            };
            let (scope, master) = match self.prompt.ask(&asking) {
                Said::Allow { scope, master } => (scope, master),
                Said::Refuse => return self.refused_by_user(program, Why::Refused),
                Said::CouldNotAsk => return Outcome::refused(Why::CouldNotAsk),
            };
            if locked {
                let Some(master) = master else {
                    return self.refused_by_user(program, Why::Refused);
                };
                match self.vault.unlock(&master) {
                    Ok(true) => {}
                    Ok(false) => {
                        wrong = true;
                        continue;
                    }
                    Err(e) => return Outcome::refused(Why::Failed(e.kind())),
                }
            }
            self.last_used = Some((self.clock)());
            let found = candidates(self.vault.logins(), query);
            let login = match found.as_slice() {
                [] => return Outcome::refused(Why::NoLogin),
                [only] => *only,
                several => {
                    let choices: Vec<Choice<'_>> = several
                        .iter()
                        .map(|login| Choice {
                            target: login.target(),
                            username: &login.username,
                        })
                        .collect();
                    let asking = Asking {
                        locked: false,
                        wrong: false,
                        ..asking
                    };
                    let picked = match self.prompt.choose(&asking, &choices) {
                        Picked::Login(at) => several.get(at),
                        Picked::Nothing => None,
                        Picked::CouldNotAsk => return Outcome::refused(Why::CouldNotAsk),
                    };
                    match picked {
                        Some(login) => *login,
                        // None of them, or one not offered.
                        None => {
                            let until = (self.clock)().checked_add(QUIET);
                            if let Some(until) = until {
                                self.quiet.insert(program.exe.clone(), until);
                            }
                            return Outcome::refused(Why::Refused);
                        }
                    }
                }
            };
            let answer = login.answer();
            if scope == Scope::UntilLocked {
                self.allowed
                    .entry(program.exe.clone())
                    .or_default()
                    .insert(login.id);
            }
            return Outcome::given(answer, Why::Allowed);
        }
        self.refused_by_user(program, Why::WrongPassword)
    }

    /// Refuse `program`, and keep refusing it unasked for [`QUIET`].
    fn refused_by_user(&mut self, program: &Program, why: Why) -> Outcome {
        if let Some(until) = (self.clock)().checked_add(QUIET) {
            self.quiet.insert(program.exe.clone(), until);
        }
        Outcome::refused(why)
    }

    /// Lock the vault if it has sat unused as long as it may, and forget
    /// quiet spells that are over.
    pub fn expire(&mut self) {
        let now = (self.clock)();
        if self.next_expiry().is_some_and(|at| now >= at) {
            self.lock();
        }
        self.quiet.retain(|_, until| now < *until);
    }

    /// When [`expire`](Self::expire) will next lock the vault: `None` while
    /// this service holds it closed, or when it is to stay open for longer
    /// than a clock can count.
    #[must_use]
    pub fn next_expiry(&self) -> Option<Instant> {
        self.last_used
            .and_then(|last| last.checked_add(self.vault.stays_open()))
    }

    /// How long from now [`expire`](Self::expire) will lock the vault, by
    /// this service's clock -- what a loop waits for at most: `None` as for
    /// [`next_expiry`](Self::next_expiry), and nothing once it is due.
    #[must_use]
    pub fn until_expiry(&self) -> Option<Duration> {
        self.next_expiry()
            .map(|at| at.saturating_duration_since((self.clock)()))
    }

    /// Lock the vault now -- the session locking, the user logging out --
    /// and forget what the user allowed until it locked.
    pub fn lock(&mut self) {
        self.vault.lock();
        self.allowed.clear();
        self.last_used = None;
    }
}

/// The program at the other end of `peer`, if it may ask at all; else why
/// not, and who it was when the kernel named it.
fn identify(peer: &impl Peer) -> Result<Program, (Why, Option<Program>)> {
    let program = match peer.program() {
        Ok(Some(program)) => program,
        Ok(None) => return Err((Why::Unknown, None)),
        Err(e) => return Err((Why::Failed(e.kind()), None)),
    };
    // After the name: see the module doc's step 2.
    match peer.holds_key() {
        Ok(true) => Ok(program),
        Ok(false) => Err((Why::NoKey, Some(program))),
        Err(e) => Err((Why::Failed(e.kind()), Some(program))),
    }
}

/// The logins in `logins` matching `query` best -- none, one, or several
/// matching equally well.
fn candidates<'v>(logins: &'v [SavedLogin], query: &Query) -> Vec<&'v SavedLogin> {
    let mut best = MatchPriority::None;
    let mut found = Vec::new();
    for login in logins {
        if query
            .username
            .as_ref()
            .is_some_and(|name| *name != login.username)
        {
            continue;
        }
        let priority = match_url(login.target(), &query.target);
        if priority == MatchPriority::None || priority < best {
            continue;
        }
        if priority > best {
            best = priority;
            found.clear();
        }
        found.push(login);
    }
    found
}

/// What one connection came to, for the service's log: never the password,
/// nor which user's login was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served {
    /// How the ask ended.
    pub why: Why,
    /// Who asked, when the kernel named them.
    pub program: Option<Program>,
    /// What for, as asked.
    pub target: String,
}

/// Answer one program: read its query from `conn`, freshly accepted, and
/// write what `service` answers.
///
/// # Errors
///
/// `TimedOut` if no whole query came in [`QUERY_PATIENCE`]; `UnexpectedEof`
/// if the program hung up first; `InvalidData` if what it sent is not a
/// query, or is more than one; and the transport's own errors -- for a
/// program that hung up while the user was asked, `BrokenPipe`.
pub fn serve<T, V, P>(service: &mut Service<V, P>, mut conn: T) -> io::Result<Served>
where
    T: Transport<Error = io::Error> + Peer,
    V: Vault,
    P: Prompt,
{
    let waiting = Waiting {
        patience: Some(QUERY_PATIENCE),
        abandoned: None,
    };
    let Some(query) = svcconn::read_frame(&mut conn, protocol::decode_query, waiting)? else {
        // Nothing here gives up waiting, so this is never read.
        return Err(io::ErrorKind::Interrupted.into());
    };
    let Outcome {
        answer,
        mut why,
        program,
    } = service.answer(&conn, &query);
    let frame = match protocol::encode_answer(&answer) {
        Ok(frame) => frame,
        Err(_) => {
            why = Why::TooLong;
            protocol::encode_answer(&Answer::Refused)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        }
    };
    // The frame holds the password: overwritten once sent.
    let frame = Secret::new(frame);
    conn.write(frame.as_bytes())?;
    Ok(Served {
        why,
        program,
        target: query.target,
    })
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
