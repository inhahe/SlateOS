//! `agetty`: the command. Stage 1 of the port (`lib.rs`) -- the command
//! line, the printed answers (`--help`, `--version`, `--list-speeds`),
//! `--show-issue` and `--reload`. Running on a line is stage 2; until then
//! the binary is built only with the `port` feature, for
//! `scripts/agetty-diff.sh`.

use std::ffi::OsString;
use std::process::ExitCode;

use agetty::issue::{Paths, SystemHost};
use agetty::options::{self, Parsed};
use agetty::{Diag, Exit, System};
use ulclosestream::Stdout;

/// `AGETTY_RELOAD_FILENAME`: the file whose touch tells running agettys to
/// print the prompt again.
const RELOAD_FILE: &str = "/run/agetty.reload";

fn main() -> ExitCode {
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
                let msg = format!("{what} {RELOAD_FILE}: {}", errmsg::strerror(&e));
                sys.errx(msg.as_bytes()).0
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
                    let why = errmsg::strerror(&std::io::Error::from_raw_os_error(e));
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
        Parsed::Run(op) => agetty::run::run(op, sys).0,
        #[cfg(not(unix))]
        Parsed::Run(_) => sys.errx(b"a terminal line is a Unix thing").0,
    }
}

/// `reload_agettys ()`: create the trigger file if need be, and touch it.
/// The failure: which step, as upstream words it, and why.
fn reload_agettys() -> Result<(), (&'static str, std::io::Error)> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `S_IRUSR|S_IWUSR`.
        open.mode(0o600);
    }
    let file = open.open(RELOAD_FILE).map_err(|e| ("cannot open", e))?;
    // `futimens (fd, NULL)`: both times, now.
    let now = std::time::SystemTime::now();
    file.set_times(
        std::fs::FileTimes::new()
            .set_accessed(now)
            .set_modified(now),
    )
    .map_err(|e| ("cannot touch file", e))?;
    Ok(())
}
