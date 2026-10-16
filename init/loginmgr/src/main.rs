//! Signs people in at the login screen and starts their desktop as them.
//!
//! The crate's documentation (`lib.rs`) says what it is and why. Here are
//! its threads: one accepting the login screen's connections, one
//! per connection answering its calls, one per started program waiting for
//! it to exit, and the main thread, which starts the login screen whenever
//! nothing is running and is told of each exit. Every decision is
//! [`Manager`]'s, behind one lock.

use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use libservicebus::{Connection, Message, MessageType, ServiceHost, fields};
use loginmgr::manager::{Accounts as _, Manager, Next, Reply, automatic_account};
use loginmgr::protocol::SERVICE_NAME;
use loginmgr::system::{Database, KernelLauncher, Logind, say};

/// The manager, as the threads share it.
type Shared = Arc<Mutex<Manager<authlib::Authenticator, Database, Logind, KernelLauncher>>>;

/// Where the kernel's command line is read, for the words that mark a start
/// for repair.
const KERNEL_COMMAND_LINE: &str = "/proc/cmdline";

/// Present once someone has signed in since the machine started: a
/// `loginmgr` started again then (it failed, and init restarted it) signs no
/// one in by itself. Under `/var/run`, which a start of the machine empties,
/// as `authlib`'s tally is.
const SIGNED_IN_MARK: &str = "/var/run/loginmgr/signed-in";

/// A login screen that exits sooner than this after it started is started
/// again only after this long, so that one which cannot run is not started
/// over and over as fast as the machine can.
const SCREEN_RETRY: Duration = Duration::from_secs(5);

/// Seconds since 1970. A clock set before then reads as 1970: a proof then
/// lasts until the clock is right, which the sign-in that sets it does not
/// need.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The manager, or the end of this process: a thread panicked while it held
/// the lock, and the state it left is not one to sign anyone in from. init
/// starts `loginmgr` again.
fn lock(
    m: &Shared,
) -> MutexGuard<'_, Manager<authlib::Authenticator, Database, Logind, KernelLauncher>> {
    match m.lock() {
        Ok(g) => g,
        Err(_) => {
            say("a thread failed while deciding; stopping");
            std::process::exit(1);
        }
    }
}

/// Wait for each of `pids` on a thread of its own, and say on `exits` when
/// it has exited. One process each, never "any child": waiting for any would
/// reap a program another thread is waiting for.
fn watch(pids: Vec<u32>, exits: &Sender<u32>) {
    for pid in pids {
        let exits = exits.clone();
        std::thread::spawn(move || {
            let Ok(p) = i32::try_from(pid) else {
                return;
            };
            loop {
                match libcall::process::wait(p) {
                    Err(e) if e == libcall::EINTR => {}
                    // Exited, or no longer a child to wait for: either way
                    // it is not running.
                    _ => {
                        // Fails only once the main thread has gone, when
                        // there is no one left to tell.
                        let _ = exits.send(pid);
                        return;
                    }
                }
            }
        });
    }
}

/// Mark that someone has signed in since the machine started
/// ([`SIGNED_IN_MARK`]).
fn mark_signed_in() {
    let path = Path::new(SIGNED_IN_MARK);
    if path.exists() {
        return;
    }
    let made = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, b""));
    if let Err(e) = made {
        say(&format!("cannot mark {SIGNED_IN_MARK}: {e}"));
    }
}

/// Answer one connection's calls until it closes.
fn serve(mut conn: Connection, m: Shared, exits: Sender<u32>) {
    // Asked once: the kernel recorded it when the client connected.
    let caller = conn.peer_credentials();
    loop {
        let Ok(mut msg) = conn.recv() else {
            return;
        };
        if msg.msg_type != MessageType::MethodCall {
            libservicebus::wipe(&mut msg.payload);
            continue;
        }
        let (reply, started) = {
            let mut g = lock(&m);
            let reply = g.call(caller, &msg.member, &msg.payload);
            if g.session().is_some() {
                mark_signed_in();
            }
            (reply, g.take_started())
        };
        // `Authenticate`'s payload is a password.
        libservicebus::wipe(&mut msg.payload);
        watch(started, &exits);
        let answer = match reply {
            Reply::Return(payload) => Message::reply(&msg).with_payload(&payload),
            Reply::Error(name, None) => Message::error(&msg, name),
            Reply::Error(name, Some(words)) => {
                Message::error(&msg, name).with_payload(&fields::encode(&[words.as_bytes()]))
            }
        };
        if conn.send_blocking(&answer).is_err() {
            return;
        }
    }
}

/// Accept connections for as long as the service is registered.
fn accept(host: &ServiceHost, m: &Shared, exits: &Sender<u32>) {
    loop {
        match host.accept() {
            Ok(conn) => {
                let (m, exits) = (Arc::clone(m), exits.clone());
                std::thread::spawn(move || serve(conn, m, exits));
            }
            Err(e) => {
                say(&format!("cannot accept a connection: {e:?}"));
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// Start the login screen whenever nothing runs, and follow every exit.
fn supervise(m: &Shared, exits: &Sender<u32>, exited: &Receiver<u32>) -> ExitCode {
    loop {
        let shown = Instant::now();
        let started = {
            let mut g = lock(m);
            if let Err(why) = g.show_login_screen() {
                say(&format!("cannot start the login screen: {why}"));
                return ExitCode::FAILURE;
            }
            g.take_started()
        };
        watch(started, exits);
        loop {
            let Ok(pid) = exited.recv() else {
                return ExitCode::FAILURE;
            };
            if lock(m).exited(pid) == Next::ShowLoginScreen {
                break;
            }
        }
        if let Some(rest) = SCREEN_RETRY.checked_sub(shown.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        say("takes no arguments");
        return ExitCode::from(2);
    }
    let host = match ServiceHost::register(SERVICE_NAME) {
        Ok(h) => h,
        Err(e) => {
            say(&format!("cannot register {SERVICE_NAME}: {e:?}"));
            return ExitCode::FAILURE;
        }
    };
    let db = Database::at(Path::new(userdb::DEFAULT_PATH));
    // Unreadable, the start is taken for one for repair: signing someone in
    // by themselves on a start that may have been one is the worse mistake.
    let cmdline = std::fs::read(KERNEL_COMMAND_LINE).unwrap_or_else(|e| {
        say(&format!(
            "cannot read {KERNEL_COMMAND_LINE}: {e}; signing no one in by themselves"
        ));
        b"recovery".to_vec()
    });
    let automatic = automatic_account(&db.offered(), &cmdline, Path::new(SIGNED_IN_MARK).exists());
    let m: Shared = Arc::new(Mutex::new(Manager::new(
        authlib::Authenticator::new(),
        db,
        Logind,
        KernelLauncher,
        now,
        automatic,
    )));
    let (exits, exited) = channel();
    {
        let (m, exits) = (Arc::clone(&m), exits.clone());
        std::thread::spawn(move || accept(&host, &m, &exits));
    }
    supervise(&m, &exits, &exited)
}
