//! `clear`: ncurses 6.4's (`progs/clear.c`, 20240113), ported.
//!
//! Clears the screen of the terminal `TERM` (or `-T`) names, with the
//! terminal's `clear` -- padded for as many lines as the screen has -- and
//! then, unless `-x`, its `E3`, which clears the scrollback too. The work is
//! [`coreutils::ncurses::clear_cmd`], which `tput clear` shares.
//!
//! Unlike `tput`, `clear` hands `setupterm` nowhere to put its verdict, so
//! an unknown terminal is `setupterm`'s own complaint and exit status 1.
//!
//! Deliberately different: `-V` names SlateOS's coreutils rather than the
//! ncurses version, and a name echoed in a complaint has its unprintable
//! bytes escaped (design-decisions §370).

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses::{self as nc, Legacy, TtySettings};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd;

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const CLEAR: Program = Program::new("clear", 1);

/// `usage`'s text after its first line.
const USAGE: &[u8] = b"\n\
Options:\n\
\x20 -T TERM     use this instead of $TERM\n\
\x20 -V          print curses-version\n\
\x20 -x          do not try to clear scrollback\n";

/// `usage`: exit status 1.
fn usage(progname: &[u8]) -> u8 {
    let mut m = b"Usage: ".to_vec();
    m.extend_from_slice(escape_unprintable(progname).as_bytes());
    m.extend_from_slice(b" [options]\n");
    m.extend_from_slice(USAGE);
    ulclosestream::stderr_write(&m);
    1
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let status = run(&argv, &mut out);
    // `exit`'s own flush, whose failure nobody hears of: there is no
    // `close_stdout` here.
    out.flush_at_exit();
    ExitCode::from(status)
}

/// Upstream's `main`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let progname = nc::rootname(&argv0).to_vec();
    let mut term = std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned());
    let mut opt_x = false;
    let mut use_env = true;
    let mut use_tioctl = false;
    let mut operands = 0usize;
    for item in CLEAR
        .parse(argv.get(1..).unwrap_or_default(), "T:Vx", &[])
        .short_only(true)
    {
        match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return usage(&progname);
            }
            Ok(Opt::Short(b'T', value)) => {
                use_env = false;
                use_tioctl = true;
                term = value.map(|v| os_bytes(&v).into_owned());
            }
            Ok(Opt::Short(b'V', _)) => {
                out.write(&nc::version_line(&progname));
                return 0;
            }
            Ok(Opt::Short(b'x', _)) => opt_x = true,
            Ok(Opt::Operand(_)) => operands = operands.saturating_add(1),
            Ok(Opt::Short(..) | Opt::Long(..)) => return usage(&progname),
        }
    }
    if operands > 0 {
        return usage(&progname);
    }

    let tty = match TtySettings::save(false, &progname) {
        Ok((tty, _)) => tty,
        Err(code) => return code,
    };

    // `setupterm (term, fd, (int *) 0)`: with nowhere to put its verdict it
    // prints it and exits. `use_tioctl` is set only with `use_env` cleared,
    // so it has no `LINES` or `COLUMNS` to update.
    let opts = terminfo::Options {
        fd: tty.fd(),
        use_env,
        use_tioctl,
    };
    let tenv = terminfo::Env::from_process();
    // The name itself, even an empty one; only a missing one sends
    // `setupterm` to `TERM`, which is just as missing.
    let setup = terminfo::setupterm_with(term.as_deref(), None, &tenv, &opts);
    if let Some(complaint) = &setup.complaint {
        ulclosestream::stderr_write(&nc::setupterm_complaint(complaint));
        return 1;
    }
    let padding = setup.padding();
    let Some(entry) = setup.entry else {
        return 1;
    };
    let lines = Legacy::of(&entry).lines;
    let strings = terminfo::legacy_copy(&entry, &tenv);
    if nc::clear_cmd(out, &strings, &entry, lines, &padding, opt_x) {
        0
    } else {
        1
    }
}
