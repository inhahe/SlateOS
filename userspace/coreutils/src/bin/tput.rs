//! `tput`: ncurses 6.4's (`progs/tput.c`, 20240113), ported.
//!
//! Each operand names a capability of the terminal `TERM` (or `-T`) names:
//! a boolean answers in the exit status, a number is printed, a string is
//! written -- formatted with the operands after it that read as numbers
//! (or as strings, for the few capabilities that take them) and padded at
//! the terminal's speed. Three names are commands instead: `clear`, `init`
//! and `reset`, the last two sending the terminal's initialization strings
//! and putting its line settings right, as `tset` does. Invoked as `clear`,
//! `init` or `reset`, `tput` runs that command first. With `-S` the
//! operands come a line at a time from standard input instead.
//!
//! The shared machinery -- the line settings, the init strings, `clear` --
//! is [`coreutils::ncurses`]; the terminal's description is `terminfo`'s.
//!
//! Quirks kept, all measured against Ubuntu 24.04's `tput`:
//!
//! - A capability given operands is formatted even when it takes none, and
//!   comes out empty: `tput bold cols` prints only the columns.
//! - Operands are counted as parameters only while they read whole as
//!   non-negative decimal numbers; the rest are the next capabilities.
//!   `tput cup 0x10 010` moves to row 17, column 9 (`strtol` base 0 for the
//!   value) and then complains of a capability called `0x10` (base 10 for
//!   the count).
//! - Invoked as `clear` with options and no operands, the option words are
//!   read as capabilities after the command: `clear -x` clears and then
//!   complains of `-x`.
//! - The static variables a capability sets (`%PA`) are still set for the
//!   next: `_nc_reset_tparm (NULL)` clears the no-terminal state, not the
//!   terminal's.
//!
//! Deliberately different: `-V` names SlateOS's coreutils rather than the
//! ncurses version, and a name echoed in a complaint has its unprintable
//! bytes escaped (design-decisions §370).

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses::{self as nc, Legacy, MyFile, PutChar, Reset, TtySettings};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd;
use coreutils::stdio::StdioReader;
use libcall::termios::Termios;
use terminfo::{Arg, Entry, NUM_PARM, Padding, TiString, Tparm};

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const TPUT: Program = Program::new("tput", 2);

/// `BUFSIZ`: `-S` reads its lines in pieces of one less than this.
const BUFSIZ: usize = 8192;

/// `usage`'s text after its first line.
const USAGE: &[u8] = b"\n\
Options:\n\
\x20 -S <<       read commands from standard input\n\
\x20 -T TERM     use this instead of $TERM\n\
\x20 -V          print curses-version\n\
\x20 -v          verbose, show warnings\n\
\x20 -x          do not try to clear scrollback\n\
\n\
Commands:\n\
\x20 clear       clear the screen\n\
\x20 init        initialize the terminal\n\
\x20 reset       reinitialize the terminal\n\
\x20 capname     unlike clear/init/reset, print value for capability \"capname\"\n";

/// What ends the program early -- `quit`, `usage`, `failed` -- with the
/// complaint already written: the status to exit with.
struct Exit(u8);

/// `check_aliases`: which of the three commands a name is.
#[derive(Clone, Copy, Default)]
struct Aliases {
    init: bool,
    reset: bool,
    clear: bool,
}

impl Aliases {
    fn of(name: &[u8]) -> Self {
        Self {
            init: nc::same_program(name, b"init"),
            reset: nc::same_program(name, b"reset"),
            clear: nc::same_program(name, b"clear"),
        }
    }

    fn any(self) -> bool {
        self.init || self.reset || self.clear
    }
}

/// `TParams`: how `tparm` is to be called for a capability.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TParams {
    /// Its parameters are as its own text says, strings among them.
    Other,
    /// Numbers only.
    Numbers,
    /// A string.
    Str,
    /// A number and a string.
    NumStr,
    /// A number and two strings.
    NumStrStr,
    /// Two strings.
    StrStr,
}

/// `tparm_type (name)`: the few capabilities known to take strings, by any
/// of their names; numbers for the rest.
fn tparm_type(name: &[u8]) -> TParams {
    match name {
        b"pkey_key" | b"pfkey" | b"pk" | b"pkey_local" | b"pfloc" | b"pl" | b"pkey_xmit"
        | b"pfx" | b"px" | b"plab_norm" | b"pln" | b"pn" => TParams::NumStr,
        b"pkey_plab" | b"pfxl" | b"xl" => TParams::NumStrStr,
        b"Cs" => TParams::Str,
        b"Ms" => TParams::StrStr,
        _ => TParams::Numbers,
    }
}

/// `strtol (s, &end, base)`, and whether it read all of `s`.
fn strtol_whole(s: &[u8], base: u32) -> (i64, bool, usize) {
    let (n, used) = cstrtol::strtol(s, base);
    (n, used == s.len(), used)
}

/// What `tput_cmd` reads and writes that outlives one command.
struct Tput<'o> {
    /// `_nc_progname`.
    progname: Vec<u8>,
    /// `-v`.
    opt_v: bool,
    /// `-x`.
    opt_x: bool,
    out: &'o mut ulclosestream::Stdout,
    /// `cur_term`, as `tigetstr` reads it.
    entry: Entry,
    /// Its legacy copy, as the macros read it.
    strings: Entry,
    /// Its `short` numbers.
    legacy: Legacy,
    /// Its `tparm` state.
    tparm: Tparm,
    /// How `tputs` pads on it.
    padding: Padding,
    /// `save_tty_settings`'s.
    tty: TtySettings,
}

impl Tput<'_> {
    /// `quit (status, fmt, ...)`: `tput: MESSAGE`.
    fn quit(&self, status: u8, message: &[u8]) -> Exit {
        quit(&self.progname, status, message)
    }

    /// `putp (s)`: `tputs (s, 1, putchar)`.
    fn putp(&mut self, s: &[u8]) {
        terminfo::putp_to(s, &self.padding, &mut PutChar(self.out));
    }

    /// `tput_cmd`: the command `argv[0]` with what follows it, and how many
    /// words it used -- or the status of an exit it makes.
    fn tput_cmd(&mut self, settings: &mut Termios, argv: &[Vec<u8>]) -> Result<(u8, usize), Exit> {
        let Some(name) = argv.first() else {
            return Ok((0, 1));
        };
        let aliases = Aliases::of(name);
        let fd = self.tty.fd();
        if aliases.reset || aliases.init {
            let mut oldmode = *settings;
            if aliases.reset {
                nc::reset_tty_settings(fd, settings, false);
            }
            nc::set_window_size(fd, &mut self.legacy);
            nc::set_control_chars(settings, &self.strings, -1, -1, -1);
            nc::set_conversions(settings, &self.strings);
            let mut file = MyFile::Stdout(self.out);
            let mut reset = Reset {
                file: &mut file,
                use_reset: aliases.reset,
                use_init: !aliases.reset,
                entry: &self.entry,
                strings: &self.strings,
                legacy: &mut self.legacy,
                tparm: &mut self.tparm,
                padding: self.padding,
                progname: &self.progname,
            };
            if reset
                .send_init_strings(fd, Some(&mut oldmode), &self.tty)
                .map_err(Exit)?
            {
                // `reset_flush`.
                file.fflush();
            }
            self.tty.update(&oldmode, settings);
            return Ok((0, 1));
        }

        if name.as_slice() == b"longname" {
            self.out.write(self.entry.longname());
            return Ok((0, 1));
        }
        if name.as_slice() == b"clear" {
            let lines = self.legacy.lines;
            let cleared = nc::clear_cmd(
                self.out,
                &self.strings,
                &self.entry,
                lines,
                &self.padding,
                self.opt_x,
            );
            return Ok((if cleared { 0 } else { nc::ERR_USAGE }, 1));
        }
        let flag = self.entry.tigetflag(name);
        if flag != -1 {
            // `exit_code (BOOLEAN, status)`: true is 0.
            return Ok((u8::from(flag == 0), 1));
        }
        let number = self.entry.tigetnum(name);
        if number != terminfo::CANCELLED_NUMERIC {
            self.out.write(format!("{number}\n").as_bytes());
            return Ok((0, 1));
        }
        let s = match self.entry.tigetstr(name) {
            TiString::NotAString => {
                let mut m = b"unknown terminfo capability '".to_vec();
                m.extend_from_slice(escape_unprintable(name).as_bytes());
                m.push(b'\'');
                return Err(self.quit(nc::ERR_CAP_NAME, &m));
            }
            // `exit_code (STRING, 1)`.
            TiString::Absent => return Ok((1, 1)),
            TiString::Value(s) => s.to_vec(),
        };
        let mut used: usize = 1;
        let formatted = if argv.len() > 1 {
            let (formatted, provided) = self.format(name, &s, argv);
            used = used.saturating_add(provided);
            formatted
        } else {
            Some(s)
        };
        // "use putp() in order to perform padding"; `putp (NULL)` is `ERR`,
        // and nothing written.
        if let Some(f) = formatted {
            self.putp(&f);
        }
        Ok((0, used))
    }

    /// The parameters' half of `tput_cmd`: `s` formatted with what follows
    /// `argv[0]`, and how many of those words it counts as its parameters.
    fn format(&mut self, name: &[u8], s: &[u8], argv: &[Vec<u8>]) -> (Option<Vec<u8>>, usize) {
        let argc = argv.len();
        // "Nasty hack time": each word as a number (all of it, base 0, or
        // 0) and as a string, the ninth the last.
        let mut numbers = [0i64; NUM_PARM + 1];
        let mut strings: [Option<&[u8]>; NUM_PARM + 1] = [None; NUM_PARM + 1];
        for (k, word) in argv.iter().enumerate().take(NUM_PARM + 1).skip(1) {
            if let (Some(n), Some(t)) = (numbers.get_mut(k), strings.get_mut(k)) {
                let (value, whole, _) = strtol_whole(word, 0);
                *n = if whole { value } else { 0 };
                *t = Some(word);
            }
        }
        let num = |k: usize| numbers.get(k).copied().unwrap_or(0);
        let text = |k: usize| strings.get(k).copied().flatten();

        let mut param_type = tparm_type(name);
        // "If the capability is an extended one, analyze the string."
        if param_type == TParams::Numbers
            && !terminfo::names::STRNAMES
                .iter()
                .any(|n| n.as_bytes() == name)
        {
            param_type = TParams::Other;
        }
        let mut popcount = 0;
        // "Count the number of numeric parameters which are provided."
        let mut provided: usize = 0;
        for (narg, word) in argv.iter().enumerate().skip(1) {
            let (check, whole, used) = strtol_whole(word, 10);
            if check < 0 || used == 0 || !whole {
                break;
            }
            provided = narg;
        }
        let entry = &self.entry;
        let tparm = &mut self.tparm;
        let (formatted, mut analyzed) = match param_type {
            TParams::Str => {
                let f = tparm.tparm(entry, s, &[Arg::Str(text(1))]);
                if provided == 0 {
                    provided = provided.saturating_add(1);
                }
                (f, 1)
            }
            TParams::StrStr => {
                let f = tparm.tparm(entry, s, &[Arg::Str(text(1)), Arg::Str(text(2))]);
                if provided == 0 {
                    provided = provided.saturating_add(1);
                }
                if provided == 1 && argc >= 2 {
                    provided = provided.saturating_add(1);
                }
                (f, 2)
            }
            TParams::NumStr => {
                let f = tparm.tparm(entry, s, &[Arg::Num(num(1)), Arg::Str(text(2))]);
                if provided == 1 && argc >= 2 {
                    provided = provided.saturating_add(1);
                }
                (f, 2)
            }
            TParams::NumStrStr => {
                let args = [Arg::Num(num(1)), Arg::Str(text(2)), Arg::Str(text(3))];
                let f = tparm.tparm(entry, s, &args);
                if provided == 1 && argc >= 2 {
                    provided = provided.saturating_add(1);
                }
                if provided == 2 && argc >= 3 {
                    provided = provided.saturating_add(1);
                }
                (f, 3)
            }
            TParams::Numbers => {
                let analysis = terminfo::analyze(s);
                popcount = analysis.popped;
                // `TIPARM_9`: `_nc_tiparm (9, ...)`, which reads each `long`
                // as an `int`.
                let params: Vec<i32> = (1..=NUM_PARM).map(|k| cstrtol::low_i32(num(k))).collect();
                (
                    tparm.nc_tiparm(entry, NUM_PARM, s, &params),
                    analysis.parsed,
                )
            }
            TParams::Other => {
                let analysis = terminfo::analyze(s);
                popcount = analysis.popped;
                let args: Vec<Arg<'_>> = (1..=NUM_PARM)
                    .map(|k| {
                        if analysis
                            .is_string
                            .get(k.saturating_sub(1))
                            .copied()
                            .unwrap_or(false)
                        {
                            Arg::Str(text(k))
                        } else {
                            Arg::Num(num(k))
                        }
                    })
                    .collect();
                (tparm.tparm(entry, s, &args), analysis.parsed)
            }
        };
        if analyzed < popcount {
            analyzed = popcount;
        }
        if self.opt_v && analyzed != provided {
            let mut m = escape_unprintable(&self.progname).into_bytes();
            m.extend_from_slice(b": ");
            m.extend_from_slice(if analyzed < provided {
                b"extra"
            } else {
                b"missing"
            });
            m.extend_from_slice(b" parameters for \"");
            m.extend_from_slice(escape_unprintable(name).as_bytes());
            m.extend_from_slice(b"\"\n");
            ulclosestream::stderr_write(&m);
        }
        (formatted, provided)
    }
}

/// `quit (status, fmt, ...)`: `PROGNAME: MESSAGE`.
fn quit(progname: &[u8], status: u8, message: &[u8]) -> Exit {
    let mut m = escape_unprintable(progname).into_bytes();
    m.extend_from_slice(b": ");
    m.extend_from_slice(message);
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
    Exit(status)
}

/// `usage (optstring)`: [`usage_text`] on standard error, and `ErrUsage`.
fn usage(progname: &[u8], optstring: Option<&[u8]>) -> Exit {
    ulclosestream::stderr_write(&usage_text(progname, optstring));
    Exit(nc::ERR_USAGE)
}

/// What `usage (optstring)` prints: the whole text, or -- for the three
/// other names -- only the options `optstring` lists, cut as upstream's loop
/// cuts it, and no commands.
fn usage_text(progname: &[u8], optstring: Option<&[u8]>) -> Vec<u8> {
    let mut m = b"Usage: ".to_vec();
    m.extend_from_slice(escape_unprintable(progname).as_bytes());
    m.extend_from_slice(b" [options] [command]\n");
    match optstring {
        None => m.extend_from_slice(USAGE),
        Some(optstring) => {
            let mut s = 0usize;
            while let Some(&c) = USAGE.get(s) {
                m.push(c);
                let rest = USAGE.get(s..).unwrap_or_default();
                if rest.starts_with(b"  -") {
                    let letter = rest.get(3).copied().unwrap_or(0);
                    if !optstring.contains(&letter) {
                        // `s = strchr (s, '\n') + 1`, and then the loop's
                        // own `++s`: the next line's first byte is skipped.
                        s = rest
                            .iter()
                            .position(|&b| b == b'\n')
                            .map_or(USAGE.len(), |nl| s.saturating_add(nl).saturating_add(1));
                    }
                } else if rest.starts_with(b"\n\nC") {
                    break;
                }
                s = s.saturating_add(1);
            }
        }
    }
    m
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(&argv, &mut out) {
        Ok(code) | Err(Exit(code)) => code,
    };
    // `exit`'s own flush, whose failure nobody hears of: there is no
    // `close_stdout` here.
    out.flush_at_exit();
    ExitCode::from(status)
}

/// Upstream's `main`, up to its `ExitProgram`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> Result<u8, Exit> {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    // `check_aliases (_nc_rootname (argv[0]), TRUE)`: the name is kept
    // either way.
    let progname = nc::rootname(&argv0).to_vec();
    let alias = Aliases::of(&progname);
    let is_alias = alias.any();

    let mut term = std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned());
    let mut cmdline = true;
    let mut opt_v = false;
    let mut opt_x = false;
    let mut use_env = true;
    let mut use_tioctl = false;
    let words = argv.get(1..).unwrap_or_default();
    let shorts = if is_alias { "T:Vvx" } else { "ST:Vvx" };
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in TPUT.parse(words, shorts, &[]).short_only(true) {
        match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return Err(usage(&progname, is_alias.then_some(b"TVx".as_slice())));
            }
            Ok(Opt::Short(b'S', _)) => cmdline = false,
            Ok(Opt::Short(b'T', value)) => {
                use_env = false;
                use_tioctl = true;
                term = value.map(|v| os_bytes(&v).into_owned());
            }
            Ok(Opt::Short(b'V', _)) => {
                out.write(&nc::version_line(&progname));
                return Ok(0);
            }
            Ok(Opt::Short(b'v', _)) => opt_v = true,
            Ok(Opt::Short(b'x', _)) => opt_x = true,
            Ok(Opt::Operand(o)) => operands.push(os_bytes(o).into_owned()),
            Ok(Opt::Short(..) | Opt::Long(..)) => {
                return Err(usage(&progname, is_alias.then_some(b"TVx".as_slice())));
            }
        }
    }

    let need_tty = alias.reset
        || alias.init
        || operands
            .first()
            .is_some_and(|o| o.as_slice() == b"reset" || o.as_slice() == b"init");

    // "Modify the argument list to omit the options we processed": invoked
    // by another name, that name is the first command -- followed by the
    // operands, or with none, by every word `getopt` read.
    let commands: Vec<Vec<u8>> = if is_alias {
        let rest: Vec<Vec<u8>> = if operands.is_empty() {
            words.iter().map(|w| os_bytes(w).into_owned()).collect()
        } else {
            operands
        };
        std::iter::once(progname.clone()).chain(rest).collect()
    } else {
        operands
    };

    let term = match term {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err(quit(
                &progname,
                nc::ERR_USAGE,
                b"No value for $TERM and no -T specified",
            ));
        }
    };

    let (tty, saved) = TtySettings::save(need_tty, &progname).map_err(Exit)?;
    let old_settings = saved.unwrap_or_default();

    // `use_tioctl` is set only with `use_env` cleared, so `setupterm` has
    // no `LINES` or `COLUMNS` to update.
    let opts = terminfo::Options {
        fd: tty.fd(),
        use_env,
        use_tioctl,
    };
    let tenv = terminfo::Env::from_process();
    let setup = terminfo::setupterm_with(Some(&term), None, &tenv, &opts);
    let padding = setup.padding();
    let entry = match setup.entry {
        Some(e) if setup.complaint.is_none() || setup.status > 0 => e,
        _ => {
            let mut m = b"unknown terminal \"".to_vec();
            m.extend_from_slice(escape_unprintable(&term).as_bytes());
            m.push(b'"');
            return Err(quit(&progname, nc::ERR_TERM_TYPE, &m));
        }
    };
    let mut tput = Tput {
        progname,
        opt_v,
        opt_x,
        out,
        legacy: Legacy::of(&entry),
        strings: terminfo::legacy_copy(&entry, &tenv),
        entry,
        tparm: Tparm::new(),
        padding,
        tty,
    };

    if cmdline {
        if commands.is_empty() && !is_alias {
            return Err(usage(&tput.progname, None));
        }
        let mut code = 0;
        let mut rest = commands.as_slice();
        while !rest.is_empty() {
            let mut settings = old_settings;
            let (c, used) = tput.tput_cmd(&mut settings, rest)?;
            code = c;
            if code != 0 {
                break;
            }
            rest = rest.get(used..).unwrap_or_default();
        }
        return Ok(code);
    }

    let mut stdin = StdioReader::stdin();
    let outcome = read_commands(&mut tput, &mut stdin, old_settings);
    // What `exit` does to a stream that was read.
    stdin.exit_sync();
    outcome
}

/// `-S`: each line of standard input split at white space into a command
/// line of its own. A failing command does not stop the rest; the first
/// makes the status 5, each one after it one more.
fn read_commands(
    tput: &mut Tput<'_>,
    stdin: &mut StdioReader,
    old_settings: Termios,
) -> Result<u8, Exit> {
    let mut result: i32 = 0;
    while let Some(line) = nc::fgets(stdin, BUFSIZ) {
        // `strlen (buf)`: a NUL ends the line early.
        let line = line.split(|&b| b == 0).next().unwrap_or_default();
        let words: Vec<Vec<u8>> = line
            .split(|&b| cstrtol::isspace(b))
            .filter(|w| !w.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        let mut rest = words.as_slice();
        while !rest.is_empty() {
            let mut settings = old_settings;
            let (code, used) = tput.tput_cmd(&mut settings, rest)?;
            if code != 0 {
                if result == 0 {
                    // "will return value >4"
                    result = i32::from(nc::err_system(0));
                }
                result = result.saturating_add(1);
            }
            rest = rest.get(used..).unwrap_or_default();
        }
    }
    Ok(nc::low_byte(result))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_that_take_strings_are_known_by_every_name() {
        for name in [
            &b"pfkey"[..],
            b"pk",
            b"pkey_key",
            b"pln",
            b"pn",
            b"plab_norm",
        ] {
            assert!(tparm_type(name) == TParams::NumStr);
        }
        assert!(tparm_type(b"pfxl") == TParams::NumStrStr);
        assert!(tparm_type(b"Cs") == TParams::Str);
        assert!(tparm_type(b"Ms") == TParams::StrStr);
        assert!(tparm_type(b"cup") == TParams::Numbers);
    }

    #[test]
    fn the_usage_for_another_name_is_cut_as_upstream_cuts_it() {
        // Measured: `clear -S`, with `tput` linked as `clear`.
        let want = b"Usage: clear [options] [command]\n\nOptions:\n\
            \x20 -T TERM     use this instead of $TERM\n\
            \x20 -V          print curses-version\n\
            \x20 -x          do not try to clear scrollback\n";
        assert_eq!(usage_text(b"clear", Some(b"TVx")), want);
        // And the whole of it otherwise.
        let all = usage_text(b"tput", None);
        assert!(all.starts_with(b"Usage: tput [options] [command]\n\nOptions:\n  -S <<"));
        assert!(all.ends_with(b"print value for capability \"capname\"\n"));
    }

    #[test]
    fn a_number_must_be_all_of_its_word() {
        assert_eq!(strtol_whole(b"0x10", 0), (16, true, 4));
        assert_eq!(strtol_whole(b"0x10", 10), (0, false, 1));
        assert_eq!(strtol_whole(b" 5", 10), (5, true, 2));
        assert_eq!(strtol_whole(b"5 ", 10), (5, false, 1));
        assert_eq!(strtol_whole(b"", 10), (0, true, 0));
    }
}
