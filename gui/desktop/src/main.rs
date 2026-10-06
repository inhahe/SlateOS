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
//! One of the session's outputs is *drained by the caller on purpose* --
//! [`ShellSession::take_launches`], the programs the user asked to start,
//! power actions included (`powerctl` is a program) -- because policy about
//! how a program starts belongs outside the window manager. `run` never
//! yields between pumps, so anything the user launched under it is queued and
//! never started.
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
    println!("  --display ADDR   the compositor to connect to: HOST:PORT, or");
    println!("                   service:NAME for a SlateOS service");
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

/// Where the desktop looked for its compositor, in words a person can act
/// on: the display it was given; else the one `SLATE_DISPLAY` names
/// (`variable`, read only when the variable is set); else the default --
/// which on SlateOS is the display service, tried before the TCP address.
fn where_looked(
    given: Option<&str>,
    variable: Option<std::io::Result<String>>,
    on_slateos: bool,
) -> String {
    use guiremote::socket::DEFAULT_DISPLAY;
    if let Some(given) = given {
        return given.to_owned();
    }
    match variable {
        Some(Ok(named)) => named,
        // Set, and unreadable: the error that follows says why.
        Some(Err(_)) => format!("the display {} names", oswindow::DISPLAY_VAR),
        None if on_slateos => format!(
            "the default display (the service {}, then {DEFAULT_DISPLAY})",
            guiremote::channel::DISPLAY_SERVICE
        ),
        None => DEFAULT_DISPLAY.to_owned(),
    }
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
    // `connect_to_display`, which takes what SLATE_DISPLAY takes -- an address,
    // or `service:NAME` for a SlateOS service -- so `--display` and the
    // variable cannot come to mean different things.
    let link = match args.display.as_deref() {
        Some(display) => oswindow::connect_to_display(display),
        None => oswindow::connect(),
    };
    let link = match link {
        Ok(link) => link,
        Err(e) => {
            // Where it looked, named rather than described: "connection
            // refused" without it is the one diagnostic a user cannot act on,
            // because the display may have come from SLATE_DISPLAY -- which is
            // exactly the thing they cannot see.
            let looked = where_looked(
                args.display.as_deref(),
                std::env::var_os(oswindow::DISPLAY_VAR).map(|_| guiremote::socket::display_addr()),
                cfg!(all(target_os = "linux", target_vendor = "slateos")),
            );
            eprintln!("desktop: cannot reach the compositor at {looked}: {e}");
            return ExitCode::from(1);
        }
    };

    // This is the desktop: its sounds are heard, from the first -- the
    // session's start says so aloud. Nothing else that builds a shell is.
    desktop::event_sounds::allow_playback();
    // `start_for_user`, not `start`: the user's appearance, widgets and clock
    // are read here or not at all -- `start` leaves them to its caller.
    let mut session = match ShellSession::start_for_user(EventLoop::new(link)) {
        Ok(session) => session,
        Err(e) => {
            // A refused surface or a failed first paint: either way there is
            // no desktop, and the error says which.
            eprintln!("desktop: the session could not start: {e}");
            return ExitCode::from(1);
        }
    };

    // The volume controls are the sound card's from here on, or say why they
    // cannot be (design-decisions 1485). The desktop's alone, as its sounds
    // are: nothing else that builds a shell turns the machine's volume.
    session
        .shell_mut()
        .attach_volume(desktop::volume::Output::open());
    // And the brightness the kernel reports: shown, not yet settable.
    session
        .shell_mut()
        .attach_backlight(desktop::backlight::Source::kernel());

    // Every window hears when a settings file changes (design-decisions
    // 1418). With no configuration directory there is nothing to watch, and
    // nothing any program could have saved either.
    if let Some(dir) = appearance::config::config_dir() {
        session.watch_settings(dir);
    }

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
        drain(&mut session);
        if !session.events_mut().connection().is_open() {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// Carry out the one intent the session deliberately cannot: starting a
/// program. The power buttons, the start menu's and the login screen's, are
/// launches of `powerctl` and arrive here the same way.
///
/// **A failed launch is reported, not swallowed.** The start menu names
/// programs by path, and on a development host most of those paths do not
/// exist. Saying so is the difference between "this desktop cannot start
/// anything" and "that entry is wrong", and only the second is actionable.
fn drain<T: guiremote::client::Transport>(session: &mut ShellSession<T>) {
    for launch in session.take_launches() {
        // `args`, not a path with spaces in it. `SCREENSHOT_COMMAND` was
        // "/usr/bin/screenshot --fullscreen" until 2026-09-17 and arrived here
        // as one `PathBuf`, so this line asked the operating system for a file
        // with a space and two dashes in its name. Both screenshot shortcuts
        // failed at every press, and said so politely enough that it read like
        // a missing program rather than a malformed request.
        // The whole command line, arguments and all: "cannot start
        // /bin/powerctl suspend" says which button failed, where the program
        // alone would read the same for four of them.
        let mut command = Command::new(&launch.program);
        command.args(&launch.args);
        // Where the launch says -- a file's folder, for an item of its
        // right-click menu -- and otherwise where the desktop is.
        if let Some(dir) = &launch.dir {
            command.current_dir(dir);
        }
        if let Err(e) = command.spawn() {
            eprintln!("desktop: cannot start {}: {e}", launch.display_line());
            // And on the screen, which is where the person who asked is
            // looking: the Run box comes back on the line, anything else is
            // a notification.
            session.report_failed_launch(&launch, &e);
        }
    }
    // An installed program's entry that could not be used is a program
    // missing from the menu with no word why -- except this one.
    for problem in session.take_app_problems() {
        eprintln!(
            "desktop: {}: not in the menu: {}",
            pathcodec::display_os(problem.path.as_os_str()),
            problem.why
        );
    }
    // And a service menu, or an item of one, missing from a file's
    // right-click menu (design-decisions 1448).
    for (path, why) in session.take_service_menu_problems() {
        eprintln!(
            "desktop: {}: not offered in file menus: {why}",
            pathcodec::display_os(path.as_os_str())
        );
    }
}
#[cfg(test)]
mod tests {
    use super::{Wanted, decide, where_looked};

    /// **A compositor that cannot be reached is named where it was looked
    /// for**: the display given, the one `SLATE_DISPLAY` names, or the
    /// default -- on SlateOS, the service before the address
    /// (`requests/f-c-the-desktops-display-argument-should-take-a-service-too.md`).
    #[test]
    fn where_it_looked_is_said() {
        let named = || Some(Ok(String::from("service:org.example.Second")));
        assert_eq!(
            where_looked(Some("10.0.0.2:7373"), named(), true),
            "10.0.0.2:7373"
        );
        assert_eq!(
            where_looked(None, named(), true),
            "service:org.example.Second"
        );
        let unreadable = Some(Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)));
        assert_eq!(
            where_looked(None, unreadable, false),
            "the display SLATE_DISPLAY names"
        );
        assert_eq!(
            where_looked(None, None, true),
            "the default display (the service org.slateos.Display, then 127.0.0.1:7373)"
        );
        assert_eq!(where_looked(None, None, false), "127.0.0.1:7373");
    }

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
