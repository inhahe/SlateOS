//! The desktop shell.
//!
//! Dials the compositor, starts a [`ShellSession`], and runs it until the
//! connection closes.
//!
//! # Why this binary exists
//!
//! Until 2026-09-13 it did not, and `src/main.rs` was the scripted demo now at
//! `src/bin/demo.rs`. [`ShellSession`] — 1 300 lines, four surfaces, a login
//! screen, hotkeys, animations and a full test suite — was described in its own
//! documentation as "the real loop, and it is what a live session runs", and
//! `ShellSession::start` was called from **nothing but its own tests**. So the
//! taskbar, the start menu, the calendar and every other surface in
//! `gui/desktop` were reachable only by a unit test calling them directly.
//!
//! That is the same defect `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` describes for
//! applications, which was closed for all 135 of them earlier the same day. The
//! shell was the one client left that could not connect, and it is the client
//! every other one is drawn on top of.
//!
//! # Why this is not `ShellSession::run`
//!
//! [`ShellSession::run`] exists and loops correctly, and a shell cannot use it.
//! Two of the session's outputs are *drained by the caller on purpose* —
//! [`ShellSession::take_launches`] and [`ShellSession::take_login_power`] —
//! because policy about how a program starts, or how a machine turns off,
//! belongs outside the window manager. `run` never yields between pumps, so
//! anything the user launched under it is queued and never started.
//!
//! So this drives [`ShellSession::pump`] itself and drains after each turn.
//! `run` stays for a caller with nothing to drain, which is every test.

use desktop::session::ShellSession;
use oswindow::EventLoop;
use oswindow::app::Args;
use std::process::{Command, ExitCode};

/// What to print when asked, and what to refuse.
///
/// `--display` is the one option, and it is the same one every application
/// takes: a second display on one machine should not require editing the
/// environment of the first.
fn usage() {
    println!("usage: desktop [--display ADDR]");
    println!();
    println!("The desktop shell: taskbar, start menu, calendar and login screen.");
    println!("Connects to the compositor named by --display, or by SLATE_DISPLAY,");
    println!("or to the default local display.");
    println!();
    println!("  --display ADDR   the compositor to connect to");
    println!("  -h, --help       this message");
    println!("  -V, --version    version");
}

/// What the command line asks for, before anything is dialled.
///
/// A separate decision from acting on it so that it can be tested: everything
/// after this point needs a compositor on the other end of a socket, and this
/// is the half that decides whether to dial at all. `apps/imageviewer` and the
/// games split theirs the same way, for the same reason.
#[derive(Debug, PartialEq, Eq)]
enum Wanted {
    /// Start the desktop.
    Run,
    /// Print the usage and stop.
    Help,
    /// Print the version and stop.
    Version,
    /// Refuse, naming the argument that was not understood.
    Refuse(String),
}

/// Read [`Wanted`] off the arguments, which do **not** include the program name.
///
/// `--help` and `--version` win over everything, including a bad argument
/// beside them: someone who typed both is asking what the options are, and
/// answering with a refusal would withhold exactly that.
fn decide(args: &[String]) -> Wanted {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Wanted::Help;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        return Wanted::Version;
    }
    // `--display ADDR` is two arguments, and the value may look like anything
    // -- including a leading dash, for a host that starts with one. So the
    // option and the thing after it are both consumed before anything is
    // judged, rather than every unrecognised-looking token being refused.
    let mut rest = args.iter();
    while let Some(a) = rest.next() {
        if a == "--display" {
            let _ = rest.next();
            continue;
        }
        if let Some(inline) = a.strip_prefix("--display=") {
            let _ = inline;
            continue;
        }
        return Wanted::Refuse(a.clone());
    }
    Wanted::Run
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match decide(&raw) {
        Wanted::Run => {}
        Wanted::Help => {
            usage();
            return ExitCode::SUCCESS;
        }
        Wanted::Version => {
            println!("desktop {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Wanted::Refuse(bad) => {
            eprintln!("desktop: unrecognized argument: {bad:?}");
            usage();
            return ExitCode::from(2);
        }
    }

    let args = match Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("desktop: {e}");
            return ExitCode::from(2);
        }
    };
    let link = match args.display.as_deref() {
        Some(addr) => oswindow::connect_to(addr),
        None => oswindow::connect(),
    };
    let link = match link {
        Ok(link) => link,
        Err(e) => {
            // The *resolved* address, named rather than described.
            // "Connection refused" without it is the one diagnostic a user
            // cannot act on, because the address may have come from
            // SLATE_DISPLAY -- which is exactly the thing they cannot see.
            let where_ = match args.display.clone() {
                Some(addr) => addr,
                None => guiremote::socket::display_addr()
                    .unwrap_or_else(|_| String::from("the default display")),
            };
            eprintln!("desktop: cannot reach the compositor at {where_}: {e}");
            return ExitCode::from(1);
        }
    };

    let mut session = match ShellSession::start(EventLoop::new(link)) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("desktop: the compositor refused a shell surface: {e}");
            return ExitCode::from(1);
        }
    };

    loop {
        match session.pump() {
            Ok(busy) => {
                if !busy && session.events_mut().wait().is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("desktop: {e}");
                return ExitCode::from(1);
            }
        }
        if !drain(&mut session) {
            break;
        }
        if !session.events_mut().connection().is_open() {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// Carry out the two intents the session deliberately cannot.
///
/// Answers whether the shell should keep running: a power action ends it.
///
/// **A failed launch is reported, not swallowed.** The start menu names
/// programs by path, and on a development host most of those paths do not
/// exist. Saying so is the difference between "this desktop cannot start
/// anything" and "that entry is wrong", and only the second is actionable.
fn drain<T: guiremote::client::Transport>(session: &mut ShellSession<T>) -> bool {
    for path in session.take_launches() {
        if let Err(e) = Command::new(&path).spawn() {
            eprintln!("desktop: cannot start {}: {e}", path.display());
        }
    }
    // Logging rather than acting, for now: there is no channel to whatever
    // turns the machine off, and inventing one in the shell would put the
    // policy in the window manager. `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`
    // covers the same gap for programs.
    if let Some(action) = session.take_login_power() {
        eprintln!("desktop: {action:?} requested; no power service to ask, so exiting");
        return false;
    }
    true
}
#[cfg(test)]
mod tests {
    use super::{Wanted, decide};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    /// A shell takes no files and no subject, so anything but `--display` is a
    /// user who expected something to happen.
    ///
    /// Starting the desktop anyway would be a wrong answer delivered
    /// confidently: the session comes up, looks right, and is not what was
    /// asked for.
    #[test]
    fn only_display_is_accepted() {
        assert_eq!(decide(&args(&[])), Wanted::Run);
        assert_eq!(decide(&args(&["--display", "1.2.3.4:9"])), Wanted::Run);
        assert_eq!(decide(&args(&["--display=1.2.3.4:9"])), Wanted::Run);
        assert_eq!(
            decide(&args(&["--bogus"])),
            Wanted::Refuse(String::from("--bogus"))
        );
        assert_eq!(
            decide(&args(&["session.yaml"])),
            Wanted::Refuse(String::from("session.yaml"))
        );
    }

    /// An address is a value, not an option, even when it looks like one.
    ///
    /// A host that starts with a dash is legal, and refusing it because the
    /// token begins `-` would make the option unusable for exactly the
    /// addresses that most need spelling out.
    ///
    /// **`--display --help` is the deliberate exception**, and this test
    /// asserted the opposite until it failed. `--help` is scanned for before
    /// the option walk, so it wins even in the value position. That is the
    /// better answer for the case that actually happens -- a forgotten
    /// address -- and it costs nothing real, because no host is named
    /// `--help`. The rule is worth stating rather than discovering.
    #[test]
    fn the_address_after_display_is_never_judged() {
        assert_eq!(decide(&args(&["--display", "--weird-host:1"])), Wanted::Run);
        assert_eq!(decide(&args(&["--display", "-h0st:1"])), Wanted::Run);
        assert_eq!(
            decide(&args(&["--display", "--help"])),
            Wanted::Help,
            "a forgotten address is far likelier than a host called --help"
        );
    }

    /// Asking what the options are is answered, not refused.
    ///
    /// Someone who typed a bad argument *and* `--help` is asking which
    /// arguments exist. Refusing would withhold the one thing that would fix
    /// their command.
    #[test]
    fn help_and_version_win_over_a_bad_argument() {
        assert_eq!(decide(&args(&["--bogus", "--help"])), Wanted::Help);
        assert_eq!(decide(&args(&["--bogus", "--version"])), Wanted::Version);
        assert_eq!(decide(&args(&["--help", "--version"])), Wanted::Help);
    }
}
