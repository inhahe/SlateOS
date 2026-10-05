//! Tests for the service's judgement, against a stand-in kernel record, a
//! stand-in vault and a scripted user.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unchecked_time_subtraction
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guiremote::client::Transport;

use super::{
    Asking, Choice, Outcome, Peer, Picked, Program, Prompt, QUIET, Said, SavedLogin, Scope,
    Service, TRIES, Vault, Why, serve,
};
use crate::protocol::{self, Answer, Decoded, MAX_USERNAME, Query, Secret};

/// The vault's master password in these tests.
const MASTER: &[u8] = b"correct horse";

/// How long the stand-in vault stays open unused.
const STAYS_OPEN: Duration = Duration::from_mins(5);

// --- The kernel's record --------------------------------------------------

/// A connection's other end, as the stand-in kernel recorded it. An error
/// is kept as its kind, since `io::Error` cannot be copied out.
#[derive(Debug)]
struct Asker {
    program: io::Result<Option<Program>>,
    key: io::Result<bool>,
}

impl Asker {
    /// The program at `exe`, holding the key.
    fn holding(exe: &str) -> Self {
        Self {
            program: Ok(Some(program(exe))),
            key: Ok(true),
        }
    }

    fn without_key(exe: &str) -> Self {
        Self {
            key: Ok(false),
            ..Self::holding(exe)
        }
    }
}

impl Peer for Asker {
    fn program(&self) -> io::Result<Option<Program>> {
        match &self.program {
            Ok(program) => Ok(program.clone()),
            Err(e) => Err(e.kind().into()),
        }
    }

    fn holds_key(&self) -> io::Result<bool> {
        match &self.key {
            Ok(key) => Ok(*key),
            Err(e) => Err(e.kind().into()),
        }
    }
}

fn program(exe: &str) -> Program {
    Program {
        pid: 77,
        exe: PathBuf::from(exe),
    }
}

// --- The vault ------------------------------------------------------------

/// The stand-in vault's state, shared with the test.
#[derive(Debug)]
struct Held {
    open: bool,
    unlocks: usize,
    unreadable: bool,
    stays_open: Duration,
}

/// The test's handle on the stand-in vault's state.
#[derive(Debug, Clone)]
struct VaultState(Rc<RefCell<Held>>);

impl VaultState {
    fn is_open(&self) -> bool {
        self.0.borrow().open
    }
}

/// A stand-in vault: its logins, and its state.
#[derive(Debug)]
struct TestVault {
    logins: Vec<SavedLogin>,
    state: VaultState,
}

impl TestVault {
    fn holding(logins: Vec<SavedLogin>) -> Self {
        let state = VaultState(Rc::new(RefCell::new(Held {
            open: false,
            unlocks: 0,
            unreadable: false,
            stays_open: STAYS_OPEN,
        })));
        Self { logins, state }
    }
}

/// A login to `target` for `username`, with a password naming both.
fn login(id: u64, target: &str, username: &str) -> SavedLogin {
    SavedLogin {
        id,
        url: target.into(),
        site: String::new(),
        username: username.into(),
        password: Secret::new(format!("pw-{id}").into_bytes()),
    }
}

impl Vault for TestVault {
    fn is_locked(&self) -> bool {
        !self.state.is_open()
    }

    fn unlock(&mut self, master: &Secret) -> io::Result<bool> {
        let mut held = self.state.0.borrow_mut();
        if held.unreadable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        held.unlocks += 1;
        held.open = master.as_bytes() == MASTER;
        Ok(held.open)
    }

    fn lock(&mut self) {
        self.state.0.borrow_mut().open = false;
    }

    fn logins(&self) -> &[SavedLogin] {
        if self.state.is_open() {
            &self.logins
        } else {
            &[]
        }
    }

    fn stays_open(&self) -> Duration {
        self.state.0.borrow().stays_open
    }
}

// --- The user -------------------------------------------------------------

/// What the scripted user was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    exe: PathBuf,
    target: String,
    username: Option<String>,
    locked: bool,
    wrong: bool,
}

/// A scripted user: says what it is told to, and records what it was asked.
#[derive(Debug, Default)]
struct Script {
    says: VecDeque<Said>,
    picks: VecDeque<Picked>,
    shown: Vec<Shown>,
    offered: Vec<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Default)]
struct User(Rc<RefCell<Script>>);

impl User {
    /// Will say `said`, in turn.
    fn will(&self, said: impl IntoIterator<Item = Said>) {
        self.0.borrow_mut().says.extend(said);
    }

    /// Will pick `pick` when asked to choose.
    fn will_pick(&self, pick: Picked) {
        self.0.borrow_mut().picks.push_back(pick);
    }

    fn shown(&self) -> Vec<Shown> {
        self.0.borrow().shown.clone()
    }

    fn times_asked(&self) -> usize {
        self.0.borrow().shown.len()
    }
}

fn shown(asking: &Asking<'_>) -> Shown {
    Shown {
        exe: asking.program.exe.clone(),
        target: asking.target.into(),
        username: asking.username.map(Into::into),
        locked: asking.locked,
        wrong: asking.wrong,
    }
}

impl Prompt for User {
    fn ask(&mut self, asking: &Asking<'_>) -> Said {
        let mut script = self.0.borrow_mut();
        script.shown.push(shown(asking));
        script
            .says
            .pop_front()
            .expect("the user was asked more than the test said")
    }

    fn choose(&mut self, _asking: &Asking<'_>, choices: &[Choice<'_>]) -> Picked {
        let mut script = self.0.borrow_mut();
        script.offered.push(
            choices
                .iter()
                .map(|c| (c.target.to_owned(), c.username.to_owned()))
                .collect(),
        );
        script
            .picks
            .pop_front()
            .expect("the user was asked to choose unexpectedly")
    }
}

/// Allow, typing `master`.
fn allow(scope: Scope, master: &[u8]) -> Said {
    Said::Allow {
        scope,
        master: Some(Secret::new(master.to_vec())),
    }
}

// --- The clock and the service --------------------------------------------

#[derive(Debug, Clone)]
struct Clock(Arc<Mutex<Instant>>);

impl Clock {
    fn pass(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

struct Rig {
    service: Service<TestVault, User>,
    vault: VaultState,
    user: User,
    clock: Clock,
}

fn rig(logins: Vec<SavedLogin>) -> Rig {
    let test_vault = TestVault::holding(logins);
    let vault = test_vault.state.clone();
    let user = User::default();
    let clock = Clock(Arc::new(Mutex::new(Instant::now())));
    let reading = clock.clone();
    let service =
        Service::new(test_vault, user.clone()).with_clock(move || *reading.0.lock().unwrap());
    Rig {
        service,
        vault,
        user,
        clock,
    }
}

fn ask_for(target: &str) -> Query {
    Query {
        target: target.into(),
        username: None,
    }
}

/// The login `outcome` gave, by its password; `None` for a refusal.
fn given(outcome: &Outcome) -> Option<String> {
    match &outcome.answer {
        Answer::Login { password, .. } => {
            Some(String::from_utf8(password.as_bytes().to_vec()).unwrap())
        }
        Answer::Refused => None,
    }
}

fn bank() -> Vec<SavedLogin> {
    vec![login(1, "https://bank.example", "ann")]
}

// --- Tests ----------------------------------------------------------------

/// **A program the kernel does not vouch for is refused, and the user is not
/// asked**: no key, no program on record, or a kernel that could not say.
#[test]
fn a_program_without_the_key_is_refused_unasked() {
    let mut r = rig(bank());
    let query = ask_for("https://bank.example");
    let cases = [
        (Asker::without_key("/bin/app"), Why::NoKey),
        (
            Asker {
                program: Ok(None),
                key: Ok(true),
            },
            Why::Unknown,
        ),
        (
            Asker {
                program: Err(io::ErrorKind::PermissionDenied.into()),
                key: Ok(true),
            },
            Why::Failed(io::ErrorKind::PermissionDenied),
        ),
        (
            Asker {
                key: Err(io::ErrorKind::Other.into()),
                ..Asker::holding("/bin/app")
            },
            Why::Failed(io::ErrorKind::Other),
        ),
    ];
    for (asker, why) in cases {
        let outcome = r.service.answer(&asker, &query);
        assert_eq!((given(&outcome), outcome.why), (None, why));
    }
    assert_eq!(r.user.times_asked(), 0);
    assert!(!r.vault.is_open());
}

/// **The user is asked -- shown the program and what it asked for -- and a
/// locked vault opens only with the master password typed there.** Allowed
/// once, the next ask is asked again.
#[test]
fn the_user_is_asked_and_the_vault_opens_with_their_password() {
    let mut r = rig(bank());
    let asker = Asker::holding("/bin/mail");
    let query = Query {
        target: "https://bank.example/login".into(),
        username: Some("ann".into()),
    };
    r.user.will([allow(Scope::Once, MASTER)]);
    let outcome = r.service.answer(&asker, &query);
    assert_eq!(
        (given(&outcome), outcome.why),
        (Some("pw-1".into()), Why::Allowed)
    );
    assert!(r.vault.is_open());
    assert_eq!(
        r.user.shown(),
        [Shown {
            exe: "/bin/mail".into(),
            target: "https://bank.example/login".into(),
            username: Some("ann".into()),
            locked: true,
            wrong: false,
        }]
    );
    // Once is once: asked again, now with the vault open.
    r.user.will([Said::Allow {
        scope: Scope::Once,
        master: None,
    }]);
    let again = r.service.answer(&asker, &query);
    assert_eq!(given(&again).as_deref(), Some("pw-1"));
    assert_eq!(r.user.times_asked(), 2);
    assert!(!r.user.shown()[1].locked);
}

/// **A wrong master password is asked for again, up to [`TRIES`] times** --
/// then refused, and the program quieted.
#[test]
fn a_wrong_master_password_is_asked_again_so_many_times() {
    let mut r = rig(bank());
    let asker = Asker::holding("/bin/mail");
    let query = ask_for("bank.example");
    r.user.will([
        allow(Scope::Once, b"wrong"),
        allow(Scope::Once, b"wronger"),
        allow(Scope::Once, MASTER),
    ]);
    let outcome = r.service.answer(&asker, &query);
    assert_eq!(given(&outcome).as_deref(), Some("pw-1"));
    let wrong: Vec<bool> = r.user.shown().iter().map(|s| s.wrong).collect();
    assert_eq!(wrong, [false, true, true]);

    let mut r = rig(bank());
    r.user
        .will((0..TRIES).map(|_| allow(Scope::Once, b"wrong")));
    let outcome = r.service.answer(&asker, &query);
    assert_eq!((given(&outcome), outcome.why), (None, Why::WrongPassword));
    assert_eq!(r.vault.0.borrow().unlocks, TRIES);
    assert!(!r.vault.is_open());
    let quiet = r.service.answer(&asker, &query);
    assert_eq!(quiet.why, Why::Quiet);
}

/// **A refusal quiets the program for a while**: it is refused unasked
/// until [`QUIET`] has passed -- and no other program is.
#[test]
fn a_refusal_quiets_the_program_for_a_while() {
    let mut r = rig(bank());
    let pest = Asker::holding("/bin/pest");
    let query = ask_for("bank.example");
    r.user.will([Said::Refuse]);
    let outcome = r.service.answer(&pest, &query);
    assert_eq!((given(&outcome), outcome.why), (None, Why::Refused));
    r.clock.pass(QUIET - Duration::from_secs(1));
    assert_eq!(r.service.answer(&pest, &query).why, Why::Quiet);
    assert_eq!(r.user.times_asked(), 1);
    // Another program is asked about.
    r.user.will([Said::Refuse]);
    let other = r.service.answer(&Asker::holding("/bin/other"), &query);
    assert_eq!(other.why, Why::Refused);
    assert_eq!(r.user.times_asked(), 2);
    // And once the spell is over, the pest is asked about again.
    r.clock.pass(Duration::from_secs(1));
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&pest, &query).why, Why::Refused);
    assert_eq!(r.user.times_asked(), 3);
}

/// **A user who could not be asked has refused nothing**: the program is
/// refused, saying why, and asks again at once -- not quieted.
#[test]
fn a_user_who_could_not_be_asked_refused_nothing() {
    let mut r = rig(bank());
    let mail = Asker::holding("/bin/mail");
    let query = ask_for("bank.example");
    r.user.will([Said::CouldNotAsk]);
    let outcome = r.service.answer(&mail, &query);
    assert_eq!((given(&outcome), outcome.why), (None, Why::CouldNotAsk));
    r.user.will([allow(Scope::Once, MASTER)]);
    assert_eq!(r.service.answer(&mail, &query).why, Why::Allowed);
    assert_eq!(r.user.times_asked(), 2);
}

/// **Allowing with no master password typed, while the vault is locked, is
/// a refusal** -- the vault is not touched.
#[test]
fn allowing_a_locked_vault_without_its_password_is_a_refusal() {
    let mut r = rig(bank());
    r.user.will([Said::Allow {
        scope: Scope::UntilLocked,
        master: None,
    }]);
    let outcome = r
        .service
        .answer(&Asker::holding("/bin/mail"), &ask_for("bank.example"));
    assert_eq!((given(&outcome), outcome.why), (None, Why::Refused));
    assert_eq!(r.vault.0.borrow().unlocks, 0);
}

/// **A vault that cannot be read refuses**, saying why.
#[test]
fn a_vault_that_cannot_be_read_refuses() {
    let mut r = rig(bank());
    r.vault.0.borrow_mut().unreadable = true;
    r.user.will([allow(Scope::Once, MASTER)]);
    let outcome = r
        .service
        .answer(&Asker::holding("/bin/mail"), &ask_for("bank.example"));
    assert_eq!(
        (given(&outcome), outcome.why),
        (None, Why::Failed(io::ErrorKind::PermissionDenied))
    );
}

/// **"Until locked" is not asked again until the vault locks** -- for that
/// program and that login only; locking, by time or by hand, forgets it.
#[test]
fn until_locked_is_not_asked_again_until_the_vault_locks() {
    let mut r = rig(vec![
        login(1, "https://bank.example", "ann"),
        login(2, "https://bank.example", "bob"),
    ]);
    let mail = Asker::holding("/bin/mail");
    let ann = Query {
        target: "https://bank.example".into(),
        username: Some("ann".into()),
    };
    let bob = Query {
        username: Some("bob".into()),
        ..ann.clone()
    };
    r.user.will([allow(Scope::UntilLocked, MASTER)]);
    assert_eq!(r.service.answer(&mail, &ann).why, Why::Allowed);
    let again = r.service.answer(&mail, &ann);
    assert_eq!(
        (given(&again), again.why),
        (Some("pw-1".into()), Why::AllowedEarlier)
    );
    assert_eq!(r.user.times_asked(), 1);
    // Not another login,
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&mail, &bob).why, Why::Refused);
    // nor another program.
    r.user.will([Said::Refuse]);
    let other = r.service.answer(&Asker::holding("/bin/other"), &ann);
    assert_eq!(other.why, Why::Refused);
    assert_eq!(r.user.times_asked(), 3);

    // Asked without a user name, the one login allowed is the one given,
    // though another matches as well -- and though the user refused this
    // program a moment ago: a quiet spell stops prompts, and takes back
    // nothing allowed until the vault locks.
    let either = ask_for("https://bank.example");
    let outcome = r.service.answer(&mail, &either);
    assert_eq!(
        (given(&outcome), outcome.why),
        (Some("pw-1".into()), Why::AllowedEarlier)
    );

    // The vault locks once unused for as long as it stays open...
    r.clock.pass(STAYS_OPEN);
    r.service.expire();
    assert!(!r.vault.is_open());
    r.user.will([allow(Scope::UntilLocked, MASTER)]);
    assert_eq!(r.service.answer(&mail, &ann).why, Why::Allowed);
    assert!(r.user.shown().last().unwrap().locked);
    // ... and when locked by hand, what was allowed goes with it.
    r.service.lock();
    assert!(!r.vault.is_open());
    r.user.will([allow(Scope::Once, MASTER)]);
    assert_eq!(r.service.answer(&mail, &ann).why, Why::Allowed);
    assert_eq!(r.user.times_asked(), 5);
}

/// **What was allowed until locked goes when the vault is found locked**,
/// however it came to be: allowing it again needs the master password.
#[test]
fn what_was_allowed_goes_with_a_vault_found_locked() {
    let mut r = rig(bank());
    let mail = Asker::holding("/bin/mail");
    let query = ask_for("bank.example");
    r.user.will([allow(Scope::UntilLocked, MASTER)]);
    assert_eq!(r.service.answer(&mail, &query).why, Why::Allowed);
    // Locked behind the service's back, then opened again.
    r.vault.0.borrow_mut().open = false;
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&mail, &query).why, Why::Refused);
    r.vault.0.borrow_mut().open = true;
    r.clock.pass(QUIET);
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&mail, &query).why, Why::Refused);
    assert_eq!(r.user.times_asked(), 3);
}

/// **The vault stays open only as long as it may sit unused**, each use
/// starting the wait again.
#[test]
fn the_vault_stays_open_only_so_long_unused() {
    let mut r = rig(bank());
    let mail = Asker::holding("/bin/mail");
    let query = ask_for("bank.example");
    assert_eq!(r.service.next_expiry(), None);
    r.user.will([allow(Scope::UntilLocked, MASTER)]);
    r.service.answer(&mail, &query);
    let opened = *r.clock.0.lock().unwrap();
    assert_eq!(r.service.next_expiry(), Some(opened + STAYS_OPEN));
    // Used again just before it would lock: it stays open as long again.
    r.clock.pass(STAYS_OPEN - Duration::from_secs(1));
    r.service.expire();
    assert!(r.vault.is_open());
    assert_eq!(r.service.answer(&mail, &query).why, Why::AllowedEarlier);
    r.clock.pass(STAYS_OPEN - Duration::from_secs(1));
    r.service.expire();
    assert!(r.vault.is_open());
    r.clock.pass(Duration::from_secs(1));
    r.service.expire();
    assert!(!r.vault.is_open());
    assert_eq!(r.service.next_expiry(), None);
    // A vault to stay open longer than a clock counts never locks.
    r.vault.0.borrow_mut().stays_open = Duration::MAX;
    r.user.will([allow(Scope::Once, MASTER)]);
    r.service.answer(&mail, &query);
    assert_eq!(r.service.next_expiry(), None);
    r.clock.pass(Duration::from_secs(1_000_000));
    r.service.expire();
    assert!(r.vault.is_open());
}

/// **The best login is given, and a tie is the user's to break**: a stored
/// path beats a domain; equals are offered to the user, never with their
/// passwords; picking none, or one not offered, refuses.
#[test]
fn the_best_login_is_given_and_a_tie_is_the_users_to_break() {
    let logins = vec![
        login(1, "example.com", "ann"),
        login(2, "https://example.com/mail", "ann"),
        login(3, "https://example.com/mail", "bob"),
        login(4, "other.example", "ann"),
    ];
    let mail = Asker::holding("/bin/mail");
    let query = Query {
        target: "https://example.com/mail/inbox".into(),
        username: Some("ann".into()),
    };
    let mut r = rig(logins.clone());
    r.user.will([allow(Scope::Once, MASTER)]);
    assert_eq!(
        given(&r.service.answer(&mail, &query)).as_deref(),
        Some("pw-2")
    );

    // Two equally good: the user picks.
    let either = ask_for("https://example.com/mail/inbox");
    r.user.will([Said::Allow {
        scope: Scope::Once,
        master: None,
    }]);
    r.user.will_pick(Picked::Login(1));
    assert_eq!(
        given(&r.service.answer(&mail, &either)).as_deref(),
        Some("pw-3")
    );
    assert_eq!(
        r.user.0.borrow().offered,
        [vec![
            ("https://example.com/mail".to_owned(), "ann".to_owned()),
            ("https://example.com/mail".to_owned(), "bob".to_owned()),
        ]]
    );
    // Picking one not offered refuses, and quiets.
    r.user.will([Said::Allow {
        scope: Scope::Once,
        master: None,
    }]);
    r.user.will_pick(Picked::Login(2));
    let outcome = r.service.answer(&mail, &either);
    assert_eq!((given(&outcome), outcome.why), (None, Why::Refused));
    assert_eq!(r.service.answer(&mail, &either).why, Why::Quiet);
    // Picking none refuses.
    let mut r = rig(logins.clone());
    r.user.will([allow(Scope::UntilLocked, MASTER)]);
    r.user.will_pick(Picked::Nothing);
    let outcome = r.service.answer(&mail, &either);
    assert_eq!((given(&outcome), outcome.why), (None, Why::Refused));
    // ... and allows nothing until locked.
    r.clock.pass(QUIET);
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&mail, &query).why, Why::Refused);
    // A user who could not be asked to pick refused nothing: not quieted.
    let mut r = rig(logins);
    r.user.will([allow(Scope::Once, MASTER)]);
    r.user.will_pick(Picked::CouldNotAsk);
    let outcome = r.service.answer(&mail, &either);
    assert_eq!((given(&outcome), outcome.why), (None, Why::CouldNotAsk));
    r.user.will([Said::Refuse]);
    assert_eq!(r.service.answer(&mail, &either).why, Why::Refused);
}

/// **A target the vault holds nothing for is refused** -- unasked while the
/// vault is open; asked while it is locked, since nothing can be known
/// before it opens.
#[test]
fn a_target_the_vault_holds_nothing_for_is_refused() {
    let mut r = rig(bank());
    let mail = Asker::holding("/bin/mail");
    let nothing = ask_for("https://nowhere.example");
    r.user.will([allow(Scope::Once, MASTER)]);
    let outcome = r.service.answer(&mail, &nothing);
    assert_eq!((given(&outcome), outcome.why), (None, Why::NoLogin));
    assert!(r.vault.is_open());
    let unasked = r.service.answer(&mail, &nothing);
    assert_eq!(unasked.why, Why::NoLogin);
    assert_eq!(r.user.times_asked(), 1);
    // A user name the vault has no login for is nothing too.
    let stranger = Query {
        target: "https://bank.example".into(),
        username: Some("eve".into()),
    };
    assert_eq!(r.service.answer(&mail, &stranger).why, Why::NoLogin);
    // A login with no address is for its name.
    let mut named = login(5, "", "ann");
    named.site = "work-vpn".into();
    let mut r = rig(vec![named]);
    r.user.will([allow(Scope::Once, MASTER)]);
    let outcome = r.service.answer(&mail, &ask_for("work-vpn"));
    assert_eq!(given(&outcome).as_deref(), Some("pw-5"));
}

// --- One connection -------------------------------------------------------

/// A connection to the service: what the program sent, its kernel record,
/// and what the service wrote back.
#[derive(Debug)]
struct Conn {
    incoming: VecDeque<u8>,
    hung_up: bool,
    asker: Asker,
    written: Rc<RefCell<Vec<u8>>>,
}

impl Conn {
    fn sending(bytes: &[u8], asker: Asker) -> (Self, Rc<RefCell<Vec<u8>>>) {
        let written = Rc::default();
        let conn = Self {
            incoming: bytes.iter().copied().collect(),
            hung_up: false,
            asker,
            written: Rc::clone(&written),
        };
        (conn, written)
    }
}

impl Transport for Conn {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let count = self.incoming.len();
        buf.extend(self.incoming.drain(..));
        Ok(count)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.written.borrow_mut().extend_from_slice(bytes);
        Ok(())
    }

    fn is_open(&self) -> bool {
        !(self.hung_up && self.incoming.is_empty())
    }
}

impl Peer for Conn {
    fn program(&self) -> io::Result<Option<Program>> {
        self.asker.program()
    }

    fn holds_key(&self) -> io::Result<bool> {
        self.asker.holds_key()
    }
}

fn answer_in(written: &RefCell<Vec<u8>>) -> Answer {
    let bytes = written.borrow();
    match protocol::decode_answer(&bytes) {
        Decoded::Complete(answer, used) if used == bytes.len() => answer,
        other => panic!("not one answer: {other:?}"),
    }
}

/// **One connection is one query read and one answer written.**
#[test]
fn a_connection_is_answered() {
    let mut r = rig(bank());
    let query = protocol::encode_query(&ask_for("bank.example")).unwrap();
    let (conn, written) = Conn::sending(&query, Asker::holding("/bin/mail"));
    r.user.will([allow(Scope::Once, MASTER)]);
    assert_eq!(serve(&mut r.service, conn).unwrap(), Why::Allowed);
    assert_eq!(
        answer_in(&written),
        Answer::Login {
            username: "ann".into(),
            password: Secret::new(b"pw-1".to_vec()),
        }
    );
    let (conn, written) = Conn::sending(&query, Asker::without_key("/bin/mail"));
    assert_eq!(serve(&mut r.service, conn).unwrap(), Why::NoKey);
    assert_eq!(answer_in(&written), Answer::Refused);
}

/// **A login the protocol cannot carry is refused, not cut**, and the
/// service says why.
#[test]
fn a_login_too_long_to_send_is_refused() {
    let long = login(1, "bank.example", &"a".repeat(MAX_USERNAME + 1));
    let mut r = rig(vec![long]);
    let query = protocol::encode_query(&ask_for("bank.example")).unwrap();
    let (conn, written) = Conn::sending(&query, Asker::holding("/bin/mail"));
    r.user.will([allow(Scope::Once, MASTER)]);
    assert_eq!(serve(&mut r.service, conn).unwrap(), Why::TooLong);
    assert_eq!(answer_in(&written), Answer::Refused);
}

/// **What is not one query is not answered**: bytes that are no query, and
/// a program that hangs up part way.
#[test]
fn what_is_not_one_query_is_not_answered() {
    let mut r = rig(bank());
    let (conn, written) = Conn::sending(b"\x05\0\0\0nope!", Asker::holding("/bin/mail"));
    let err = serve(&mut r.service, conn).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(written.borrow().is_empty());
    let query = protocol::encode_query(&ask_for("bank.example")).unwrap();
    let (mut conn, written) = Conn::sending(&query[..query.len() - 1], Asker::holding("/a"));
    conn.hung_up = true;
    let err = serve(&mut r.service, conn).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    assert!(written.borrow().is_empty());
    assert_eq!(r.user.times_asked(), 0);
}
