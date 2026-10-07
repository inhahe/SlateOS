//! Tests for the service's loop, against a stand-in listener whose waits
//! move a stand-in clock, so the vault's locking is seen as it happens.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use credentials::protocol::{self, Answer, Decoded, Query, Secret};
use credentials::service::{
    Asking, Choice, Peer, Picked, Program, Prompt, Said, SavedLogin, Scope, Served, Service, Vault,
    Why,
};
use guiremote::client::Transport;

use super::{Listener, dropped, record, run};

const MASTER: &[u8] = b"correct horse";
const STAYS_OPEN: Duration = Duration::from_mins(5);

// --- Stand-ins ------------------------------------------------------------

/// A vault holding one login for `bank.example`, its state shared.
#[derive(Debug)]
struct TestVault {
    logins: Vec<SavedLogin>,
    open: Rc<RefCell<bool>>,
}

impl Vault for TestVault {
    fn is_locked(&self) -> bool {
        !*self.open.borrow()
    }

    fn unlock(&mut self, master: &Secret) -> io::Result<bool> {
        let right = master.as_bytes() == MASTER;
        *self.open.borrow_mut() = right;
        Ok(right)
    }

    fn lock(&mut self) {
        *self.open.borrow_mut() = false;
    }

    fn logins(&self) -> &[SavedLogin] {
        if *self.open.borrow() {
            &self.logins
        } else {
            &[]
        }
    }

    fn stays_open(&self) -> Duration {
        STAYS_OPEN
    }
}

/// A user who allows every ask once, typing the right master password.
#[derive(Debug, Default)]
struct Allowing;

impl Prompt for Allowing {
    fn ask(&mut self, _asking: &Asking<'_>) -> Said {
        Said::Allow {
            scope: Scope::Once,
            master: Some(Secret::new(MASTER.to_vec())),
        }
    }

    fn choose(&mut self, _asking: &Asking<'_>, _choices: &[Choice<'_>]) -> Picked {
        Picked::Nothing
    }
}

/// One program's connection: what it sends, who the kernel says it is, and
/// what it was answered.
#[derive(Debug)]
struct Conn {
    incoming: VecDeque<u8>,
    hung_up: bool,
    program: Program,
    key: bool,
    answered: Rc<RefCell<Vec<u8>>>,
}

impl Transport for Conn {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let count = self.incoming.len();
        buf.extend(self.incoming.drain(..));
        Ok(count)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.answered.borrow_mut().extend_from_slice(bytes);
        Ok(())
    }

    fn is_open(&self) -> bool {
        !(self.hung_up && self.incoming.is_empty())
    }
}

impl Peer for Conn {
    fn program(&self) -> io::Result<Option<Program>> {
        Ok(Some(self.program.clone()))
    }

    fn holds_key(&self) -> io::Result<bool> {
        Ok(self.key)
    }
}

/// A connection from `/bin/<name>` sending `bytes`, and where its answer
/// will be.
fn conn(name: &str, key: bool, bytes: &[u8]) -> (Conn, Rc<RefCell<Vec<u8>>>) {
    let answered = Rc::default();
    let conn = Conn {
        incoming: bytes.iter().copied().collect(),
        hung_up: true,
        program: Program {
            pid: 77,
            exe: PathBuf::from(format!("/bin/{name}")),
        },
        key,
        answered: Rc::clone(&answered),
    };
    (conn, answered)
}

fn query(target: &str) -> Vec<u8> {
    protocol::encode_query(&Query {
        target: target.into(),
        username: None,
    })
    .unwrap()
}

fn answer_in(written: &RefCell<Vec<u8>>) -> Answer {
    let bytes = written.borrow();
    match protocol::decode_answer(&bytes) {
        Decoded::Complete(answer, _) => answer,
        other => panic!("not an answer: {other:?}"),
    }
}

/// A listener with connections queued, which -- once they are taken --
/// stops the loop after `waits_left` waits. Each wait moves the clock on by
/// what it was asked to wait, and is recorded.
struct Queue {
    pending: VecDeque<Conn>,
    waits_left: usize,
    waited: Vec<Option<Duration>>,
    clock: Arc<Mutex<Instant>>,
    stop: Arc<AtomicBool>,
    fail: bool,
}

impl Listener for Queue {
    type Conn = Conn;

    fn accept(&mut self) -> io::Result<Option<Conn>> {
        if self.fail {
            return Err(io::ErrorKind::ConnectionReset.into());
        }
        Ok(self.pending.pop_front())
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.waited.push(timeout);
        if let Some(by) = timeout {
            *self.clock.lock().unwrap() += by;
        }
        self.waits_left = self.waits_left.saturating_sub(1);
        if self.waits_left == 0 {
            self.stop.store(true, Ordering::Release);
        }
        Ok(())
    }
}

struct Rig {
    queue: Queue,
    service: Service<TestVault, Allowing>,
    open: Rc<RefCell<bool>>,
    stop: Arc<AtomicBool>,
}

fn rig(conns: Vec<Conn>, waits: usize) -> Rig {
    let clock = Arc::new(Mutex::new(Instant::now()));
    let stop = Arc::new(AtomicBool::new(false));
    let open = Rc::new(RefCell::new(false));
    let vault = TestVault {
        logins: vec![SavedLogin {
            id: 1,
            url: "https://bank.example".into(),
            site: String::new(),
            username: "ann".into(),
            password: Secret::new(b"pw-1".to_vec()),
        }],
        open: Rc::clone(&open),
    };
    let reading = Arc::clone(&clock);
    let service = Service::new(vault, Allowing).with_clock(move || *reading.lock().unwrap());
    Rig {
        queue: Queue {
            pending: conns.into(),
            waits_left: waits,
            waited: Vec::new(),
            clock,
            stop: Arc::clone(&stop),
            fail: false,
        },
        service,
        open,
        stop,
    }
}

// --- Tests ----------------------------------------------------------------

/// **Every connection is answered, in turn, and logged** -- one JSON line
/// each, saying who asked, for what, and how it ended, at a level that
/// fits; a connection that is no query is logged and the next still
/// served; and no password is ever written down.
#[test]
fn every_connection_is_answered_and_logged() {
    let (mail, mail_answer) = conn("mail", true, &query("https://bank.example/login"));
    let (garbage, garbage_answer) = conn("junk", true, b"\x05\0\0\0nope!");
    let (sneak, sneak_answer) = conn("sneak", false, &query("bank.example"));
    let mut r = rig(vec![mail, garbage, sneak], 1);
    let mut log = Vec::new();
    run(&mut r.queue, &mut r.service, &r.stop, &mut log).unwrap();

    assert_eq!(
        answer_in(&mail_answer),
        Answer::Login {
            username: "ann".into(),
            password: Secret::new(b"pw-1".to_vec()),
        }
    );
    assert!(garbage_answer.borrow().is_empty());
    assert_eq!(answer_in(&sneak_answer), Answer::Refused);

    let log = String::from_utf8(log).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "{log}");
    for want in [
        "\"level\":\"notice\"",
        "\"service\":\"credentials\"",
        "\"pid\":77",
        "\"domain\":\"bank.example\"",
        "\"outcome\":\"allowed\"",
        "\"program\":\"/bin/mail\"",
        "/bin/mail asked for a password for bank.example: given",
    ] {
        assert!(lines[0].contains(want), "{want} in {}", lines[0]);
    }
    assert!(lines[1].contains("\"outcome\":\"dropped\""), "{}", lines[1]);
    assert!(lines[1].contains("\"level\":\"warning\""), "{}", lines[1]);
    assert!(lines[2].contains("\"outcome\":\"no-key\""), "{}", lines[2]);
    assert!(lines[2].contains("\"level\":\"warning\""), "{}", lines[2]);
    assert!(
        lines[2].contains("\"program\":\"/bin/sneak\""),
        "{}",
        lines[2]
    );
    assert!(!log.contains("pw-1"), "{log}");
    assert!(!log.contains("ann"), "{log}");
}

/// **The vault is locked once it has sat unused as long as it may**: the
/// loop waits no longer than that while it is open, locks it on waking, and
/// then waits for as long as it takes.
#[test]
fn the_vault_is_locked_when_it_has_sat_unused() {
    let (mail, _) = conn("mail", true, &query("bank.example"));
    let mut r = rig(vec![mail], 2);
    let mut log = Vec::new();
    run(&mut r.queue, &mut r.service, &r.stop, &mut log).unwrap();
    assert_eq!(r.queue.waited, [Some(STAYS_OPEN), None]);
    assert!(!*r.open.borrow());
    // Nothing to answer: the loop only waits, and is told to stop.
    let mut idle = rig(Vec::new(), 1);
    run(&mut idle.queue, &mut idle.service, &idle.stop, &mut log).unwrap();
    assert_eq!(idle.queue.waited, [None]);
}

/// **A loop told to stop does not start**, and stops between connections.
#[test]
fn a_loop_told_to_stop_stops() {
    let (mail, answered) = conn("mail", true, &query("bank.example"));
    let mut r = rig(vec![mail], 5);
    r.stop.store(true, Ordering::Release);
    let mut log = Vec::new();
    run(&mut r.queue, &mut r.service, &r.stop, &mut log).unwrap();
    assert!(answered.borrow().is_empty());
    assert!(log.is_empty());
}

/// A log that refuses every write.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// **The service stops when its listener fails, and when it cannot log what
/// it gave out** -- it does not go on giving out passwords unrecorded.
#[test]
fn a_failed_listener_or_log_stops_the_service() {
    let mut r = rig(Vec::new(), 5);
    r.queue.fail = true;
    let err = run(&mut r.queue, &mut r.service, &r.stop, &mut Vec::new()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
    let (mail, _) = conn("mail", true, &query("bank.example"));
    let (later, later_answer) = conn("later", true, &query("bank.example"));
    let mut r = rig(vec![mail, later], 5);
    let err = run(&mut r.queue, &mut r.service, &r.stop, &mut Broken).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
    assert!(later_answer.borrow().is_empty());
}

/// **A record says what it can**: who asked when the kernel named nobody,
/// the error a failure was, and the time it was given.
#[test]
fn a_record_says_what_it_can() {
    let failed = Served {
        why: Why::Failed(io::ErrorKind::PermissionDenied),
        program: None,
        target: "work-vpn".into(),
    };
    let line = record(&failed, 1_700_000_000);
    for want in [
        "\"ts\":1700000000",
        "\"level\":\"err\"",
        "\"outcome\":\"failed\"",
        "\"error\":\"permission denied\"",
        "\"domain\":\"work-vpn\"",
        "a program the kernel could not name asked for a password for work-vpn",
    ] {
        assert!(line.contains(want), "{want} in {line}");
    }
    assert!(!line.contains("\"pid\""), "{line}");
    assert!(!line.contains("\"program\""), "{line}");
    let line = dropped(&io::ErrorKind::TimedOut.into(), 5);
    assert!(line.contains("\"ts\":5"), "{line}");
    assert!(line.contains("given up on unanswered: timed out"), "{line}");
}
