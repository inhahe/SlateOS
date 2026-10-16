//! `agetty` -- open a terminal line, ask for a login name, and run `login`.
//!
//! The command: the printed answers (`--help`, `--version`,
//! `--list-speeds`), `--show-issue` and `--reload` here, and a run on a
//! line in `run.rs`. util-linux 2.39.3's, ported (`lib.rs`). Until it
//! ships in `getty`'s place the binary is built only with the `port`
//! feature, for `scripts/agetty-diff.sh`.

use std::ffi::OsString;
use std::process::ExitCode;

use agetty::issue::{Paths, SystemHost};
use agetty::options::{self, Parsed};
use agetty::{Diag, Exit, RELOAD_FILE, System, strerror};
use ulclosestream::Stdout;

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    // Before anything else: a standard descriptor agetty was started without
    // is closed again, and `SIGPIPE` given back the disposition it came
    // with -- the runtime ignores it, and `login`, and the shell after it,
    // would otherwise inherit it ignored.
    stdfdguard::restore();
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_else(|| OsString::from("agetty"));
    let args: Vec<OsString> = argv.collect();
    let mut sys = System::new(&quoting::os_bytes(&argv0));
    // `setlocale (LC_ALL, "")`: `\d`'s day and month names are the
    // locale's. A locale that does not exist leaves "C", as upstream's call
    // does.
    let mut name = [0u8; 64];
    let _ = libcall::locale::select_from_env(libcall::locale::LC_ALL, &mut name);
    ExitCode::from(run(&args, &mut sys))
}

/// The program after `main`'s setup: its exit status.
fn run(args: &[OsString], sys: &mut System) -> u8 {
    // Before the arguments are parsed, as upstream's `main` does.
    #[cfg(unix)]
    let ignored = agetty::run::ignore_signals();
    let short = sys.short.clone();
    let parsed = match options::parse_args(args, &short, sys) {
        Ok(p) => p,
        Err(Exit(status)) => return status,
    };
    match parsed {
        Parsed::Print(text) => {
            // agetty registers no `close_stdout`: glibc's `exit` flushes,
            // and a failure goes unheard.
            let mut out = Stdout::new(1);
            out.write(&text);
            out.flush_at_exit();
            0
        }
        Parsed::Reload => match reload_agettys() {
            Ok(()) => 0,
            Err((what, e)) => {
                let why = strerror(e);
                let msg: [&[u8]; 5] = [
                    what.as_bytes(),
                    b" ",
                    RELOAD_FILE.to_bytes(),
                    b": ",
                    why.as_bytes(),
                ];
                sys.errx(&msg.concat()).0
            }
        },
        Parsed::ShowIssue(op) => {
            // `tcgetattr (STDIN_FILENO)`: the speed `\b` shows.
            let tp = match libcall::termios::get_attr(0) {
                Ok(tp) => tp,
                Err(e) => {
                    // `err (EXIT_FAILURE, "...: %m")`: `err` adds the reason
                    // to a message that already ends in it, so it is said
                    // twice.
                    let why = strerror(e);
                    let msg = format!("failed to get terminal attributes: {why}: {why}");
                    return sys.errx(msg.as_bytes()).0;
                }
            };
            let speed = libcall::termios::input_speed(&tp);
            let text = agetty::issue::show_issue(&op, speed, &Paths::default(), &SystemHost, sys);
            // `write_all (STDOUT_FILENO, ...)`, unbuffered; a failure is
            // not looked at.
            let mut out = Stdout::new(1);
            out.write(&text);
            out.flush_at_exit();
            0
        }
        #[cfg(unix)]
        Parsed::Run(op) => agetty::run::run(op, sys, ignored).0,
        #[cfg(not(unix))]
        Parsed::Run(_) => sys.errx(b"a terminal line is a Unix thing").0,
    }
}

/// `reload_agettys ()`: create the trigger file if need be (`S_IRUSR |
/// S_IWUSR`), and touch it -- `futimens (fd, NULL)`, which needs only
/// write permission, where setting given times needs the file's owner.
/// The failure: which step, as upstream words it, and the `errno`.
fn reload_agettys() -> Result<(), (&'static str, i32)> {
    let fd = libcall::fd::open_create_write(RELOAD_FILE, 0o600).map_err(|e| ("cannot open", e))?;
    let touched = libcall::fd::touch_now(fd);
    let closed = libcall::fd::close(fd);
    touched.and(closed).map_err(|e| ("cannot touch file", e))
}
