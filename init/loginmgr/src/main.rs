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
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Holds this start of the machine's stamp ([`this_start`]) once someone has
/// signed in since it started: a `loginmgr` started again then (it failed,
/// and init restarted it) signs no one in by itself. A mark holding another
/// start's stamp is a previous start's: nothing empties `/var/run` when the
/// machine starts (lane D's init empties only utmp), so the mark cannot rely
/// on vanishing.
const SIGNED_IN_MARK: &str = "/var/run/loginmgr/signed-in";

/// The login records, whose boot record names this start of the machine.
const UTMP: &str = "/var/run/utmp";

/// This start of the machine: the time of the last boot record init wrote
/// to utmp when it started (`seconds.microseconds`), or nothing when there
/// is none to read -- and then a mark holding nothing counts, as it did
/// before marks held anything.
fn this_start() -> Vec<u8> {
    let Ok(records) = std::fs::read(UTMP) else {
        return Vec::new();
    };
    utmpfile::parse(&records)
        .iter()
        .rev()
        .find(|r| r.record_type == utmpfile::BOOT_TIME)
        .map(|r| format!("{}.{:06}", r.tv_sec, r.login_usec).into_bytes())
        .unwrap_or_default()
}

/// Whether someone has signed in since this start of the machine.
fn signed_in_this_start() -> bool {
    std::fs::read(SIGNED_IN_MARK).is_ok_and(|mark| mark == this_start())
}

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

/// Whether this process has marked a sign-in: once is enough.
static MARKED: AtomicBool = AtomicBool::new(false);

/// Mark that someone has signed in since the machine started
/// ([`SIGNED_IN_MARK`]), the first time this process sees a session.
fn mark_signed_in() {
    if MARKED.swap(true, Ordering::SeqCst) {
        return;
    }
    let path = Path::new(SIGNED_IN_MARK);
    let made = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, this_start()));
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
        let (reply, started, signed_in) = {
            let mut g = lock(&m);
            let reply = g.call(caller, &msg.member, &msg.payload);
            (reply, g.take_started(), g.session().is_some())
        };
        // Outside the lock: a slow file system holds up no other call.
        if signed_in {
            mark_signed_in();
        }
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
    let automatic = automatic_account(&db.offered(), &cmdline, signed_in_this_start());
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
