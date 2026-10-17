//! What `loginmgr` decides: who may call it, what each call does, and what
//! follows when the login screen or a desktop exits.
//!
//! Nothing here makes a system call. Checking a password, the account
//! database, logind and starting programs are behind [`Verifier`],
//! [`Accounts`], [`Sessions`] and [`Launcher`], so every rule is tested on
//! the host with stand-ins (`main.rs` gives the real ones).
//!
//! # The rules
//!
//! - **Only the login screen asks.** A call is answered for the screen
//!   `loginmgr` started (the kernel names the caller's pid, which must be
//!   the screen's) and for root; anyone else gets
//!   [`protocol::ERR_NOT_THE_LOGIN_SCREEN`]. A caller the kernel cannot
//!   name gets the same: assuming it is the screen would let anything that
//!   can open a channel start the automatic account's desktop.
//! - **Only the accounts offered.** `Authenticate` and every session start
//!   take only an account the screen offers ([`Accounts::offered`]: people,
//!   not root or a service). A name off the list is refused without its
//!   password being checked, so the screen cannot sign in as -- or test
//!   passwords for -- an account no one is offered.
//! - **A session needs a proof.** `StartSession(user)` starts `user`'s
//!   desktop only when the same screen process had `user`'s password
//!   accepted within [`PROOF_LIFETIME_SECS`]. A proof is spent by the start,
//!   and dropped by any other `Authenticate`, by the screen's exit, and by
//!   age.
//! - **An account with no password signs in without one**, as at a console
//!   (`login`): `authlib`'s `NoPassword` is an acceptance here. A lock screen
//!   takes the other side of that fact (design-decisions §341); a login
//!   screen offering a passwordless account is how the account was meant to
//!   be used.
//! - **The automatic account starts once.** Which account, if any, is
//!   decided before the first screen starts ([`automatic_account`]):
//!   `StartAutomatic` starts that one, the first time it is asked, and only
//!   if no session has started since `loginmgr` did. After a log-out the
//!   screen is started without it.
//! - **One session at a time.** This is one seat's manager.

use authlib::Outcome;
use libservicebus::{Credentials, fields};
use loginusers::Account;

use crate::protocol::{self, Verdict};

/// How long an accepted password lets the screen start that account's
/// session, in seconds. A screen starts it at once; a proof left lying
/// longer is not one a person standing there now gave.
pub const PROOF_LIFETIME_SECS: u64 = 60;

/// The words in the kernel's command line that mark a start for repair:
/// the automatic account is not signed in then (lane C's §1427 rule).
pub const REPAIR_WORDS: [&[u8]; 2] = [b"recovery", b"single"];

/// The `PATH` a session starts with: `login`'s for an ordinary user.
pub const SESSION_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// The seat `loginmgr` signs people in at.
pub const SEAT: &str = "seat0";

/// Checks a password: [`authlib::Authenticator`], or a test's stand-in.
pub trait Verifier {
    /// `authlib`'s verdict on `password` for `user`.
    fn verify(&mut self, user: &str, password: &[u8]) -> Outcome;
}

impl Verifier for authlib::Authenticator {
    fn verify(&mut self, user: &str, password: &[u8]) -> Outcome {
        self.authenticate(user, password)
    }
}

/// An account as a session needs it: whom to start the desktop as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    /// The name signed in with.
    pub username: String,
    /// The uid the desktop runs as.
    pub uid: u32,
    /// Its primary group.
    pub gid: u32,
    /// `HOME`.
    pub home: String,
    /// `SHELL`.
    pub shell: String,
}

/// The account database.
pub trait Accounts {
    /// The accounts the screen offers ([`loginusers::offered`]).
    fn offered(&self) -> Vec<Account>;
    /// `user` as the database holds it now; `None` when it has no such
    /// account, or one without a uid or gid to run as.
    fn person(&self, user: &str) -> Option<Person>;
    /// Count a sign-in for `user` at `at` (seconds since 1970). A failure is
    /// the implementation's to report: the session has started.
    fn signed_in(&self, user: &str, at: u64);
}

/// logind.
pub trait Sessions {
    /// Register a session for `person` at [`SEAT`]: its id, or why not.
    ///
    /// # Errors
    ///
    /// logind refused or could not be reached: the words to say why.
    fn create(&mut self, person: &Person) -> Result<String, String>;
    /// Name `pid` the session's leader, once the desktop has started. A
    /// failure is the implementation's to report: without a leader logind
    /// cannot signal the session (`KillSession`), and everything else about
    /// it works, so the sign-in stands.
    fn set_leader(&mut self, id: &str, pid: u32);
    /// End the session `id`.
    fn end(&mut self, id: &str);
}

/// Starts the two programs `loginmgr` runs.
pub trait Launcher {
    /// The login screen, as its own unprivileged account, told the automatic
    /// account's name when there is one to start: its pid.
    ///
    /// # Errors
    ///
    /// The screen could not be started: the words to say why.
    fn start_login_screen(&mut self, automatic: Option<&str>) -> Result<u32, String>;
    /// `person`'s desktop, as `person`, with `env` and nothing else in its
    /// environment, holding the display service's key: its pid.
    ///
    /// # Errors
    ///
    /// The desktop could not be started: the words to say why.
    fn start_desktop(&mut self, person: &Person, env: &[(String, String)]) -> Result<u32, String>;
}

/// The account to sign in by itself at this start: the one marked, when
/// exactly one is and it is not locked ([`loginusers::automatic_account`]),
/// unless the kernel was started for repair (`cmdline`, the kernel's
/// command line, holds one of [`REPAIR_WORDS`] as a word of its own) or an
/// automatic sign-in has already happened since the machine started
/// (`done_this_start`).
#[must_use]
pub fn automatic_account(
    accounts: &[Account],
    cmdline: &[u8],
    done_this_start: bool,
) -> Option<String> {
    if done_this_start || started_for_repair(cmdline) {
        return None;
    }
    let i = loginusers::automatic_account(accounts)?;
    accounts.get(i).map(|a| a.username.clone())
}

/// Whether `cmdline` holds one of [`REPAIR_WORDS`] as a whole word.
#[must_use]
pub fn started_for_repair(cmdline: &[u8]) -> bool {
    cmdline
        .split(u8::is_ascii_whitespace)
        .any(|word| REPAIR_WORDS.contains(&word))
}

/// The environment `person`'s desktop starts with, in session `session_id`.
#[must_use]
pub fn desktop_environment(person: &Person, session_id: &str) -> Vec<(String, String)> {
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    vec![
        pair("HOME", &person.home),
        pair("USER", &person.username),
        pair("LOGNAME", &person.username),
        pair("SHELL", &person.shell),
        pair("PATH", SESSION_PATH),
        pair("XDG_RUNTIME_DIR", &format!("/run/user/{}", person.uid)),
        pair("XDG_SESSION_ID", session_id),
        pair("XDG_SESSION_CLASS", "user"),
        pair("XDG_SEAT", SEAT),
    ]
}

/// An answer to a call: its payload, or an error's name and, for
/// [`protocol::ERR_SESSION_NOT_STARTED`], the words to show.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// The call's answer.
    Return(Vec<u8>),
    /// A refusal.
    Error(&'static str, Option<String>),
}

/// Whether `payload` carries no arguments: nothing at all, or an empty
/// field list.
fn no_arguments(payload: &[u8]) -> bool {
    payload.is_empty() || fields::decode_exact(payload, 0).is_some()
}

/// What the supervisor does after a child exited.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing: the pid was not one of ours, or the screen exited because
    /// the session it asked for has started.
    Nothing,
    /// Start the login screen: the desktop exited (a log-out), or the screen
    /// exited with no session started.
    ShowLoginScreen,
}

/// An accepted password: which screen process gave it, for whom, and when.
struct Proof {
    pid: u32,
    user: String,
    at: u64,
}

/// The session that is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    /// logind's id for it.
    pub id: String,
    /// The desktop's pid.
    pub pid: u32,
    /// Whose it is.
    pub user: String,
}

/// `loginmgr`'s state, and its rules.
pub struct Manager<V, A, S, L> {
    verifier: V,
    accounts: A,
    sessions: S,
    launcher: L,
    /// Seconds since 1970, now: the system clock, or a test's.
    now: fn() -> u64,
    /// The login screen's pid while it runs.
    screen: Option<u32>,
    proof: Option<Proof>,
    /// The account `StartAutomatic` may start, until it does or any session
    /// starts.
    automatic: Option<String>,
    session: Option<Running>,
    /// The programs started since [`Manager::take_started`] was last asked,
    /// for the supervisor to wait for.
    started: Vec<u32>,
}

impl<V: Verifier, A: Accounts, S: Sessions, L: Launcher> Manager<V, A, S, L> {
    /// A manager with nothing running yet, which may start `automatic` once.
    pub fn new(
        verifier: V,
        accounts: A,
        sessions: S,
        launcher: L,
        now: fn() -> u64,
        automatic: Option<String>,
    ) -> Self {
        Self {
            verifier,
            accounts,
            sessions,
            launcher,
            now,
            screen: None,
            proof: None,
            automatic,
            session: None,
            started: Vec::new(),
        }
    }

    /// The pids of the programs started since this was last asked: the
    /// supervisor waits for each.
    pub fn take_started(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.started)
    }

    /// The login screen's pid, while it runs.
    #[must_use]
    pub const fn login_screen(&self) -> Option<u32> {
        self.screen
    }

    /// The session running, if one is.
    #[must_use]
    pub const fn session(&self) -> Option<&Running> {
        self.session.as_ref()
    }

    /// Start the login screen, naming the automatic account while there is
    /// one to start: its pid.
    ///
    /// # Errors
    ///
    /// It could not be started: the words to say why.
    pub fn show_login_screen(&mut self) -> Result<u32, String> {
        let pid = self
            .launcher
            .start_login_screen(self.automatic.as_deref())?;
        self.screen = Some(pid);
        self.started.push(pid);
        Ok(pid)
    }

    /// A child exited: what to do now.
    pub fn exited(&mut self, pid: u32) -> Next {
        if self.screen == Some(pid) {
            self.screen = None;
            self.proof = None;
            return if self.session.is_some() {
                Next::Nothing
            } else {
                Next::ShowLoginScreen
            };
        }
        match self.session.take() {
            Some(s) if s.pid == pid => {
                self.sessions.end(&s.id);
                // A log-out: the next screen is a chooser.
                self.automatic = None;
                // A desktop that ended before the screen that started it
                // exited: that screen is still on the seat, and its exit,
                // with no session now, shows the next one.
                if self.screen.is_some() {
                    Next::Nothing
                } else {
                    Next::ShowLoginScreen
                }
            }
            other => {
                self.session = other;
                Next::Nothing
            }
        }
    }

    /// Answer `method` with `payload` from `caller`, as the kernel names it
    /// (`None` when it cannot).
    pub fn call(&mut self, caller: Option<Credentials>, method: &str, payload: &[u8]) -> Reply {
        let Some(caller) = caller else {
            return Reply::Error(protocol::ERR_NOT_THE_LOGIN_SCREEN, None);
        };
        if !caller.is_root() && self.screen != Some(caller.pid) {
            return Reply::Error(protocol::ERR_NOT_THE_LOGIN_SCREEN, None);
        }
        match method {
            protocol::LIST_ACCOUNTS if no_arguments(payload) => {
                Reply::Return(protocol::encode_accounts(&self.accounts.offered()))
            }
            protocol::AUTHENTICATE => self.authenticate(caller.pid, payload),
            protocol::START_SESSION => self.start_session(caller.pid, payload),
            protocol::START_AUTOMATIC if no_arguments(payload) => match self.automatic.clone() {
                Some(user) if self.session.is_none() => self.start(&user),
                _ => Reply::Error(protocol::ERR_NO_AUTOMATIC_ACCOUNT, None),
            },
            protocol::LIST_ACCOUNTS | protocol::START_AUTOMATIC => {
                Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None)
            }
            _ => Reply::Error(protocol::ERR_UNKNOWN_METHOD, None),
        }
    }

    /// `Authenticate(user, password)`.
    fn authenticate(&mut self, pid: u32, payload: &[u8]) -> Reply {
        let Some(args) = fields::decode_exact(payload, 2) else {
            return Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None);
        };
        let (Some(user), Some(password)) = (
            args.first().and_then(|u| core::str::from_utf8(u).ok()),
            args.get(1),
        ) else {
            return Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None);
        };
        // Whatever this answer is, an earlier proof is no longer the latest
        // word from this screen.
        self.proof = None;
        let outcome = if self.is_offered(user) {
            self.verifier.verify(user, password)
        } else {
            Outcome::Rejected
        };
        let verdict = match outcome {
            Outcome::Accepted | Outcome::NoPassword => {
                self.proof = Some(Proof {
                    pid,
                    user: user.to_owned(),
                    at: (self.now)(),
                });
                Verdict::Accepted
            }
            Outcome::Rejected => Verdict::Rejected,
            Outcome::Locked => Verdict::Locked,
            Outcome::Unusable => Verdict::Unusable,
            Outcome::RateLimited { retry_after_secs } => Verdict::TryLater {
                after_secs: retry_after_secs,
            },
        };
        let message = if verdict == Verdict::Accepted {
            ""
        } else {
            outcome.user_message()
        };
        Reply::Return(protocol::encode_verdict(verdict, message))
    }

    /// `StartSession(user)`.
    fn start_session(&mut self, pid: u32, payload: &[u8]) -> Reply {
        let Some(args) = fields::decode_exact(payload, 1) else {
            return Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None);
        };
        let Some(user) = args.first().and_then(|u| core::str::from_utf8(u).ok()) else {
            return Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None);
        };
        let now = (self.now)();
        let proved = self.proof.take().is_some_and(|p| {
            p.pid == pid && p.user == user && now.saturating_sub(p.at) <= PROOF_LIFETIME_SECS
        });
        if !proved {
            return Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None);
        }
        self.start(user)
    }

    /// `user`'s session started: logind told, the desktop started as them,
    /// the sign-in counted.
    fn start(&mut self, user: &str) -> Reply {
        let not_started =
            |why: &str| Reply::Error(protocol::ERR_SESSION_NOT_STARTED, Some(why.to_owned()));
        if self.session.is_some() {
            return not_started("Someone is signed in already.");
        }
        if !self.is_offered(user) {
            return not_started("This account cannot be signed in to.");
        }
        let Some(person) = self.accounts.person(user) else {
            return not_started("This account cannot be signed in to.");
        };
        let id = match self.sessions.create(&person) {
            Ok(id) => id,
            Err(why) => return not_started(&why),
        };
        let env = desktop_environment(&person, &id);
        let pid = match self.launcher.start_desktop(&person, &env) {
            Ok(pid) => pid,
            Err(why) => {
                self.sessions.end(&id);
                return not_started(&why);
            }
        };
        self.started.push(pid);
        self.sessions.set_leader(&id, pid);
        self.accounts.signed_in(user, (self.now)());
        self.automatic = None;
        self.session = Some(Running {
            id,
            pid,
            user: user.to_owned(),
        });
        Reply::Return(Vec::new())
    }

    /// Whether the screen offers `user` now.
    fn is_offered(&self, user: &str) -> bool {
        self.accounts.offered().iter().any(|a| a.username == user)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::unnecessary_wraps
)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Everything the stand-ins were asked, in order.
    type Log = Rc<RefCell<Vec<String>>>;

    struct Passwords;
    impl Verifier for Passwords {
        fn verify(&mut self, user: &str, password: &[u8]) -> Outcome {
            match (user, password) {
                ("ada" | "bob" | "root", b"right") => Outcome::Accepted,
                ("guest" | "daemon", _) => Outcome::NoPassword,
                ("eve", _) => Outcome::Locked,
                ("slow", _) => Outcome::RateLimited {
                    retry_after_secs: 30,
                },
                _ => Outcome::Rejected,
            }
        }
    }

    struct Db;
    impl Accounts for Db {
        /// The people: ada marked to sign in by herself. root and daemon
        /// are in the database ([`Db::person`]) but not offered.
        fn offered(&self) -> Vec<Account> {
            ["ada", "bob", "guest", "eve", "slow"]
                .iter()
                .map(|&name| Account {
                    uid: Some(1000),
                    username: name.to_owned(),
                    display_name: name.to_owned(),
                    has_password: name != "guest",
                    is_admin: false,
                    auto_login: name == "ada",
                    last_login: 0,
                    avatar: None,
                    is_locked: name == "eve",
                })
                .collect()
        }
        fn person(&self, user: &str) -> Option<Person> {
            ["ada", "bob", "guest", "root", "daemon"]
                .contains(&user)
                .then(|| Person {
                    username: user.to_owned(),
                    uid: 1000,
                    gid: 1000,
                    home: format!("/home/{user}"),
                    shell: "/bin/sh".to_owned(),
                })
        }
        fn signed_in(&self, _: &str, _: u64) {}
    }

    struct Logind(Log, bool);
    impl Sessions for Logind {
        fn create(&mut self, p: &Person) -> Result<String, String> {
            self.0.borrow_mut().push(format!("create {}", p.username));
            if self.1 {
                Ok("7".to_owned())
            } else {
                Err("logind is not running".to_owned())
            }
        }
        fn set_leader(&mut self, id: &str, pid: u32) {
            self.0.borrow_mut().push(format!("leader {id} {pid}"));
        }
        fn end(&mut self, id: &str) {
            self.0.borrow_mut().push(format!("end {id}"));
        }
    }

    struct Starter(Log, u32, bool);
    impl Launcher for Starter {
        fn start_login_screen(&mut self, automatic: Option<&str>) -> Result<u32, String> {
            self.1 += 1;
            self.0
                .borrow_mut()
                .push(format!("screen {automatic:?} -> {}", self.1));
            Ok(self.1)
        }
        fn start_desktop(&mut self, p: &Person, env: &[(String, String)]) -> Result<u32, String> {
            if !self.2 {
                return Err("no desktop".to_owned());
            }
            self.1 += 1;
            let id = &env.iter().find(|(k, _)| k == "XDG_SESSION_ID").unwrap().1;
            self.0
                .borrow_mut()
                .push(format!("desktop {} session {id} -> {}", p.username, self.1));
            Ok(self.1)
        }
    }

    thread_local! {
        /// The test's clock: each test runs on a thread of its own, so one
        /// that moves its clock moves no other's.
        static NOW: std::cell::Cell<u64> = const { std::cell::Cell::new(1000) };
    }
    fn now() -> u64 {
        NOW.with(std::cell::Cell::get)
    }

    type Test = Manager<Passwords, Db, Logind, Starter>;

    fn manager(automatic: Option<&str>) -> (Test, Log) {
        manager_with(automatic, true, true)
    }

    fn manager_with(automatic: Option<&str>, logind: bool, desktop: bool) -> (Test, Log) {
        let log = Log::default();
        let m = Manager::new(
            Passwords,
            Db,
            Logind(log.clone(), logind),
            Starter(log.clone(), 100, desktop),
            now,
            automatic.map(str::to_owned),
        );
        (m, log)
    }

    fn the(pid: u32) -> Option<Credentials> {
        Some(Credentials {
            pid,
            uid: 990,
            gid: 990,
        })
    }

    fn args(a: &[&[u8]]) -> Vec<u8> {
        fields::encode(a)
    }

    fn verdict(r: &Reply) -> Verdict {
        let Reply::Return(bytes) = r else {
            panic!("{r:?}");
        };
        let answer: Vec<Vec<u8>> = fields::decode(bytes)
            .unwrap()
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        protocol::decode_verdict(&answer).unwrap().0
    }

    #[test]
    fn only_the_screen_it_started_and_root_are_answered() {
        let (mut m, _) = manager(None);
        let screen = m.show_login_screen().unwrap();
        let list = args(&[]);
        for caller in [None, the(screen + 1)] {
            assert_eq!(
                m.call(caller, protocol::LIST_ACCOUNTS, &list),
                Reply::Error(protocol::ERR_NOT_THE_LOGIN_SCREEN, None)
            );
        }
        assert!(matches!(
            m.call(the(screen), protocol::LIST_ACCOUNTS, &list),
            Reply::Return(_)
        ));
        let root = Some(Credentials {
            pid: 1,
            uid: 0,
            gid: 0,
        });
        assert!(matches!(
            m.call(root, protocol::LIST_ACCOUNTS, &list),
            Reply::Return(_)
        ));
        assert_eq!(
            m.call(the(screen), "Nope", &list),
            Reply::Error(protocol::ERR_UNKNOWN_METHOD, None)
        );
    }

    #[test]
    fn a_session_starts_for_the_account_just_accepted_and_no_other() {
        let (mut m, log) = manager(None);
        let screen = m.show_login_screen().unwrap();
        let wrong = m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"wrong"]),
        );
        assert_eq!(verdict(&wrong), Verdict::Rejected);
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None),
            "a rejected password proves nothing"
        );
        let ok = m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        assert_eq!(verdict(&ok), Verdict::Accepted);
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"bob"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None),
            "ada's password does not start bob's session"
        );
        // That attempt spent the proof.
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None)
        );
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Return(Vec::new())
        );
        assert_eq!(
            *log.borrow(),
            [
                "screen None -> 101",
                "create ada",
                "desktop ada session 7 -> 102",
                "leader 7 102"
            ]
        );
        assert_eq!(
            m.session().map(|s| (s.pid, s.user.as_str())),
            Some((102, "ada"))
        );
        assert_eq!(m.take_started(), [101, 102], "both are to be waited for");
        assert_eq!(m.take_started(), [], "once");
    }

    #[test]
    fn an_account_the_screen_does_not_offer_is_neither_checked_nor_started() {
        let (mut m, log) = manager(None);
        let screen = m.show_login_screen().unwrap();
        for (user, password) in [(&b"root"[..], &b"right"[..]), (b"daemon", b"")] {
            let answer = m.call(
                the(screen),
                protocol::AUTHENTICATE,
                &args(&[user, password]),
            );
            assert_eq!(verdict(&answer), Verdict::Rejected, "{user:?}");
            assert_eq!(
                m.call(the(screen), protocol::START_SESSION, &args(&[user])),
                Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None)
            );
        }
        assert_eq!(log.borrow().len(), 1, "no session was asked for: {log:?}");
        // Taken off the list between the password and the start.
        let (mut m, _) = manager(None);
        assert_eq!(
            m.start("root"),
            Reply::Error(
                protocol::ERR_SESSION_NOT_STARTED,
                Some("This account cannot be signed in to.".to_owned())
            )
        );
    }

    #[test]
    fn a_desktop_that_ends_before_its_screen_shows_no_second_screen() {
        let (mut m, _) = manager(None);
        let screen = m.show_login_screen().unwrap();
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"]));
        let desktop = m.session().unwrap().pid;
        assert_eq!(m.exited(desktop), Next::Nothing, "its screen is still up");
        assert!(m.session().is_none());
        assert_eq!(m.exited(screen), Next::ShowLoginScreen, "now it is gone");
    }

    #[test]
    fn a_proof_is_this_screen_s_and_does_not_last() {
        let (mut m, _) = manager(None);
        let screen = m.show_login_screen().unwrap();
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        let root = Some(Credentials {
            pid: 1,
            uid: 0,
            gid: 0,
        });
        assert_eq!(
            m.call(root, protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None),
            "another process cannot use the screen's proof"
        );
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        NOW.with(|n| n.set(n.get() + PROOF_LIFETIME_SECS + 1));
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None)
        );
        // A screen that exits takes its proof with it.
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        assert_eq!(m.exited(screen), Next::ShowLoginScreen);
        let again = m.show_login_screen().unwrap();
        assert_eq!(
            m.call(the(again), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_NOT_AUTHENTICATED, None)
        );
    }

    #[test]
    fn each_verdict_is_answered_with_its_words() {
        let (mut m, _) = manager(None);
        let screen = m.show_login_screen().unwrap();
        let mut ask = |u: &[u8]| m.call(the(screen), protocol::AUTHENTICATE, &args(&[u, b"x"]));
        assert_eq!(verdict(&ask(b"eve")), Verdict::Locked);
        assert_eq!(verdict(&ask(b"slow")), Verdict::TryLater { after_secs: 30 });
        assert_eq!(
            verdict(&ask(b"guest")),
            Verdict::Accepted,
            "no password to type"
        );
        assert_eq!(
            m.call(the(screen), protocol::AUTHENTICATE, &args(&[b"ada"])),
            Reply::Error(protocol::ERR_INVALID_ARGUMENTS, None)
        );
    }

    #[test]
    fn the_automatic_account_starts_once_and_never_after_a_log_out() {
        let (mut m, log) = manager(Some("ada"));
        let screen = m.show_login_screen().unwrap();
        assert_eq!(
            m.call(the(screen), protocol::START_AUTOMATIC, &args(&[])),
            Reply::Return(Vec::new())
        );
        assert_eq!(
            m.exited(screen),
            Next::Nothing,
            "the screen exits once the session runs"
        );
        let desktop = m.session().unwrap().pid;
        assert_eq!(m.exited(desktop), Next::ShowLoginScreen);
        let next = m.show_login_screen().unwrap();
        assert_eq!(
            m.call(the(next), protocol::START_AUTOMATIC, &args(&[])),
            Reply::Error(protocol::ERR_NO_AUTOMATIC_ACCOUNT, None)
        );
        let log = log.borrow();
        assert_eq!(log[0], "screen Some(\"ada\") -> 101");
        assert!(log.contains(&"end 7".to_owned()));
        assert_eq!(
            log.last().unwrap(),
            "screen None -> 103",
            "the screen after a log-out is a chooser"
        );
    }

    #[test]
    fn a_session_signed_in_by_password_spends_the_automatic_one() {
        let (mut m, _) = manager(Some("ada"));
        let screen = m.show_login_screen().unwrap();
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"bob", b"right"]),
        );
        m.call(the(screen), protocol::START_SESSION, &args(&[b"bob"]));
        assert_eq!(
            m.call(the(screen), protocol::START_AUTOMATIC, &args(&[])),
            Reply::Error(protocol::ERR_NO_AUTOMATIC_ACCOUNT, None)
        );
    }

    #[test]
    fn a_session_that_cannot_start_says_why_and_leaves_nothing_behind() {
        let (mut m, log) = manager_with(None, true, false);
        let screen = m.show_login_screen().unwrap();
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(
                protocol::ERR_SESSION_NOT_STARTED,
                Some("no desktop".to_owned())
            )
        );
        assert_eq!(
            log.borrow().last().unwrap(),
            "end 7",
            "logind's session ended"
        );
        assert!(m.session().is_none());
        let (mut m, _) = manager_with(None, false, true);
        let screen = m.show_login_screen().unwrap();
        m.call(
            the(screen),
            protocol::AUTHENTICATE,
            &args(&[b"ada", b"right"]),
        );
        assert_eq!(
            m.call(the(screen), protocol::START_SESSION, &args(&[b"ada"])),
            Reply::Error(
                protocol::ERR_SESSION_NOT_STARTED,
                Some("logind is not running".to_owned())
            )
        );
    }

    #[test]
    fn a_screen_that_exits_with_no_session_is_started_again() {
        let (mut m, _) = manager(None);
        let screen = m.show_login_screen().unwrap();
        assert_eq!(m.exited(screen + 50), Next::Nothing, "not ours");
        assert_eq!(m.exited(screen), Next::ShowLoginScreen);
        assert_eq!(m.login_screen(), None);
    }

    #[test]
    fn a_repair_start_or_a_second_start_signs_no_one_in() {
        let marked = Db.offered();
        assert_eq!(
            automatic_account(&marked, b"root=/dev/sda1 quiet", false),
            Some("ada".to_owned())
        );
        assert_eq!(
            automatic_account(&marked, b"root=/dev/sda1 single", false),
            None
        );
        assert_eq!(automatic_account(&marked, b"recovery", false), None);
        assert_eq!(
            automatic_account(&marked, b"singleuser=0", false),
            Some("ada".to_owned()),
            "a word, not a part of one"
        );
        assert_eq!(automatic_account(&marked, b"", true), None);
    }

    #[test]
    fn the_desktop_s_environment_names_its_session_and_nothing_inherited() {
        let p = Db.person("ada").unwrap();
        let env = desktop_environment(&p, "7");
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("HOME"), Some("/home/ada"));
        assert_eq!(get("XDG_SESSION_ID"), Some("7"));
        assert_eq!(get("XDG_RUNTIME_DIR"), Some("/run/user/1000"));
        assert_eq!(get("PATH"), Some(SESSION_PATH));
        assert_eq!(get("DISPLAY"), None);
    }
}
