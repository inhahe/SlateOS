//! `tset`, and `reset`: ncurses 6.4's (`progs/tset.c`, 20240113), ported.
//!
//! Works out the terminal's type -- the operand, else `TERM` put through
//! the `-m` mappings, else `unknown` -- asking for one when it is unknown or
//! starts with `?`; then sizes the window, chooses the erase, interrupt and
//! kill characters, sends the terminal's initialization strings (pausing a
//! second after them, for the terminal to settle) and reports the control
//! characters that changed. Invoked as `reset`, it first puts the line
//! settings back into a sane state and sends the reset strings instead. The
//! machinery is [`coreutils::ncurses`], shared with `tput init` and `tput
//! reset`.
//!
//! Quirks kept, all measured against Ubuntu 24.04's `tset`:
//!
//! - A lone `-e`, `-i` or `-k` (or one followed by an option) means `^H`,
//!   `^C`, `^U`; a lone `-` means `-q`.
//! - A `-m` baud rate is looked up in a table whose search stops where the
//!   speeds stop increasing -- at `134.5`, the same speed as `134` -- so
//!   only `0`, `50`, `75`, `110` and `134` are ever found: `-m '>9600:vt100'`
//!   is `unknown baud rate 9600`.
//! - `!` alone in a mapping inverts no test into all three, which matches
//!   nothing.
//! - A control character given as a byte past 127 is a negative `char`,
//!   which reads as "not given".
//!
//! Deliberately different: `-V` names SlateOS's coreutils rather than the
//! ncurses version, and a name echoed in a complaint has its unprintable
//! bytes escaped (design-decisions §370). And where the environment holds
//! `TERMCAP` twice, both go, not only the first.

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses::{self as nc, Legacy, MyFile, Reset, TtySettings};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd;
use coreutils::stdio::StdioReader;
use libcall::termios::Termios;
use terminfo::{Entry, Tparm};

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const TSET: Program = Program::new("tset", 1);

/// `usage`'s text after its first line.
const USAGE: &[u8] = b"\n\
Options:\n\
\x20 -c          set control characters\n\
\x20 -e ch       erase character\n\
\x20 -I          no initialization strings\n\
\x20 -i ch       interrupt character\n\
\x20 -k ch       kill character\n\
\x20 -m mapping  map identifier to type\n\
\x20 -Q          do not output control key settings\n\
\x20 -q          display term only, do no changes\n\
\x20 -r          display term on stderr\n\
\x20 -s          output TERM set command\n\
\x20 -V          print curses-version\n\
\x20 -w          set window-size\n\
\n\
If neither -c/-w are given, both are assumed.\n";

/// What ends the program early, its complaint already written: the status
/// to exit with.
struct Exit(u8);

/// Baud-rate conditionals for mapping.
const GT: u8 = 0x01;
/// Equal.
const EQ: u8 = 0x02;
/// Less than.
const LT: u8 = 0x04;
/// Inverted.
const NOT: u8 = 0x08;

/// `speeds[]`, as it compiles on Linux: the name, and the `B*` code. The
/// search stops at the first entry whose code is not above the one before.
const SPEEDS: &[(&[u8], u32)] = &[
    (b"0", 0),
    (b"50", 1),
    (b"75", 2),
    (b"110", 3),
    (b"134", 4),
    (b"134.5", 4),
    (b"150", 5),
    (b"200", 6),
    (b"300", 7),
    (b"600", 8),
    (b"1200", 9),
    (b"1800", 10),
    (b"2400", 11),
    (b"4800", 12),
    (b"9600", 13),
    (b"19200", 14),
    (b"38400", 15),
    (b"19200", 14),
    (b"38400", 15),
    (b"19200", 14),
    (b"38400", 15),
    (b"57600", 0o10001),
    (b"115200", 0o10002),
    (b"230400", 0o10003),
    (b"460800", 0o10004),
    (b"500000", 0o10005),
    (b"576000", 0o10006),
    (b"921600", 0o10007),
    (b"1000000", 0o10010),
    (b"1152000", 0o10011),
    (b"1500000", 0o10012),
    (b"2000000", 0o10013),
    (b"2500000", 0o10014),
    (b"3000000", 0o10015),
    (b"3500000", 0o10016),
    (b"4000000", 0o10017),
];

/// One `-m` mapping.
struct Map {
    /// The port type it applies to; `None` for any.
    porttype: Option<Vec<u8>>,
    /// The terminal type it selects.
    ttype: Vec<u8>,
    /// `GT`, `EQ`, `LT` -- or 0 for no test.
    conditional: u8,
    /// The `B*` code to compare the line's against.
    speed: u32,
}

/// `err (fmt, ...)` while the options are read: `tset: MESSAGE`, then
/// `exit_error` -- whose `restore_tty_settings` has nothing saved yet to
/// put back -- a newline and status 1.
fn err_early(progname: &[u8], message: &[u8]) -> Exit {
    let mut m = escape_unprintable(progname).into_bytes();
    m.extend_from_slice(b": ");
    m.extend_from_slice(message);
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
    Exit(1)
}

/// `tbaudrate (rate)`: the `B*` code of a speed, a leading `B` ignored.
fn tbaudrate(progname: &[u8], rate: &[u8]) -> Result<u32, Exit> {
    let name = rate.strip_prefix(b"B").unwrap_or(rate);
    for (n, &(string, speed)) in SPEEDS.iter().enumerate() {
        // "if the speeds are not increasing, likely a numeric overflow"
        if n > 0
            && SPEEDS
                .get(n.saturating_sub(1))
                .is_some_and(|&(_, before)| speed <= before)
        {
            break;
        }
        if name.eq_ignore_ascii_case(string) {
            return Ok(speed);
        }
    }
    let mut m = b"unknown baud rate ".to_vec();
    m.extend_from_slice(escape_unprintable(name).as_bytes());
    Err(err_early(progname, &m))
}

/// `add_mapping (port, arg)`: `[port-type][test baudrate]:terminal-type`,
/// the tests `>`, `<`, `@` (or `=`) and `!`.
fn add_mapping(progname: &[u8], port: Option<&[u8]>, arg: &[u8]) -> Result<Map, Exit> {
    let bad = || {
        let mut m = b"illegal -m option format: ".to_vec();
        m.extend_from_slice(escape_unprintable(arg).as_bytes());
        err_early(progname, &m)
    };
    let mut map = Map {
        porttype: None,
        ttype: Vec::new(),
        conditional: 0,
        speed: 0,
    };
    match arg.iter().position(|c| b"><@=!:".contains(c)) {
        // `[?]term`.
        None => map.ttype = arg.to_vec(),
        Some(start) => {
            // `[><@=! baud]:term`, a port type before it or not.
            let mut at = start;
            loop {
                match arg.get(at).copied() {
                    Some(b'<') => {
                        if map.conditional & GT != 0 {
                            return Err(bad());
                        }
                        map.conditional |= LT;
                    }
                    Some(b'>') => {
                        if map.conditional & LT != 0 {
                            return Err(bad());
                        }
                        map.conditional |= GT;
                    }
                    // `=` is "not documented".
                    Some(b'@' | b'=') => map.conditional |= EQ,
                    Some(b'!') => map.conditional |= NOT,
                    _ => break,
                }
                at = at.saturating_add(1);
            }
            let rest = arg.get(at..).unwrap_or_default();
            if let Some(after) = rest.strip_prefix(b":") {
                if map.conditional != 0 {
                    return Err(bad());
                }
                map.ttype = after.to_vec();
            } else {
                // An optional baud rate, and then the colon.
                let Some(colon) = rest.iter().position(|&c| c == b':') else {
                    return Err(bad());
                };
                let (rate, after) = rest.split_at(colon);
                map.speed = tbaudrate(progname, rate)?;
                map.ttype = after.get(1..).unwrap_or_default().to_vec();
            }
            if start > 0 {
                map.porttype = Some(arg.get(..start).unwrap_or_default().to_vec());
            }
            // "If a NOT conditional, reverse the test."
            if map.conditional & NOT != 0 {
                map.conditional = !map.conditional & (EQ | GT | LT);
            }
        }
    }
    // "If user specified a port with an option flag, set it."
    if let Some(port) = port {
        if map.porttype.is_some() {
            return Err(bad());
        }
        map.porttype = Some(port.to_vec());
    }
    Ok(map)
}

/// The program's state across its steps.
struct Tset {
    /// `_nc_progname`.
    progname: Vec<u8>,
    /// `save_tty_settings`'s.
    tty: TtySettings,
    /// `maplist`.
    maps: Vec<Map>,
    /// `ospeed`, as the line's settings give it.
    ospeed: u32,
    /// Standard input, for `askuser`.
    stdin: StdioReader,
}

impl Tset {
    /// `exit_error`: the settings put back, a newline, status 1.
    fn exit_error(&self) -> Exit {
        self.tty.restore();
        ulclosestream::stderr_write(b"\n");
        Exit(1)
    }

    /// `err (fmt, ...)`: `tset: MESSAGE`, then `exit_error`.
    fn err(&self, message: &[u8]) -> Exit {
        let mut m = escape_unprintable(&self.progname).into_bytes();
        m.extend_from_slice(b": ");
        m.extend_from_slice(message);
        ulclosestream::stderr_write(&m);
        self.exit_error()
    }

    /// `mapped (type)`: the first mapping for this port type whose test the
    /// line's speed passes, else the type itself.
    fn mapped(&self, ttype: Vec<u8>) -> Vec<u8> {
        for map in &self.maps {
            if map
                .porttype
                .as_deref()
                .is_none_or(|p| p == ttype.as_slice())
            {
                let ospeed = self.ospeed;
                let matched = match map.conditional {
                    0 => true,
                    EQ => ospeed == map.speed,
                    c if c == GT | EQ => ospeed >= map.speed,
                    GT => ospeed > map.speed,
                    c if c == LT | EQ => ospeed <= map.speed,
                    LT => ospeed < map.speed,
                    _ => false,
                };
                if matched {
                    return map.ttype.clone();
                }
            }
        }
        ttype
    }

    /// `askuser (dflt)`: a terminal type from standard input, prompting on
    /// standard error, or the default for an empty answer. With neither, the
    /// question again; at the end of the input with no default,
    /// `exit_error`.
    fn askuser(&mut self, dflt: Option<&[u8]>) -> Result<Vec<u8>, Exit> {
        // "We can get recalled; if so, don't continue uselessly." -- after
        // the `clearerr`, nothing is ever found to stop for.
        self.stdin.clear_error();
        loop {
            let mut prompt = b"Terminal type? ".to_vec();
            if let Some(d) = dflt {
                prompt.extend_from_slice(b"[");
                prompt.extend_from_slice(d);
                prompt.extend_from_slice(b"] ");
            }
            ulclosestream::stderr_write(&prompt);
            // `fgets (answer, sizeof (answer), stdin)`: 255 bytes at most.
            let Some(line) = nc::fgets(&mut self.stdin, 256) else {
                return match dflt {
                    None => Err(self.exit_error()),
                    Some(d) => Ok(d.to_vec()),
                };
            };
            // The string as C has it, up to a NUL, then up to a newline.
            let answer = line.split(|&b| b == 0).next().unwrap_or_default();
            let answer = answer.split(|&b| b == b'\n').next().unwrap_or_default();
            if !answer.is_empty() {
                return Ok(answer.to_vec());
            }
            if let Some(d) = dflt {
                return Ok(d.to_vec());
            }
        }
    }
}

/// `obsolete (argv)`: `-` is `-q`, and a lone `-e`, `-i` or `-k` that no
/// argument follows (the next word missing, or another option) takes its
/// traditional one -- in every word, `argv[0]` included.
fn obsolete(argv: &mut [Vec<u8>]) {
    for i in 0..argv.len() {
        let next_is_argument = argv
            .get(i.saturating_add(1))
            .is_some_and(|next| next.first() != Some(&b'-'));
        let Some(parm) = argv.get_mut(i) else {
            continue;
        };
        if parm.as_slice() == b"-" {
            *parm = b"-q".to_vec();
            continue;
        }
        if next_is_argument {
            continue;
        }
        let replacement: &[u8] = match parm.as_slice() {
            b"-e" => b"-e^H",
            b"-i" => b"-i^C",
            b"-k" => b"-k^U",
            _ => continue,
        };
        *parm = replacement.to_vec();
    }
}

/// `arg_to_char`: `^X` is control-X, `^?` DEL, anything else its first
/// byte -- as a `char`, which is signed, so a byte past 127 is negative and
/// reads as no character given.
fn arg_to_char(optarg: &[u8]) -> i32 {
    let c = match optarg {
        [b'^', b'?', ..] => 0o177,
        [b'^', second, ..] if *second != 0 => *second & 0o37,
        [first, ..] => *first,
        [] => 0,
    };
    i32::from(i8::from_le_bytes([c]))
}

/// `usage`: exit status 1.
fn usage(progname: &[u8]) -> Exit {
    let mut m = b"Usage: ".to_vec();
    m.extend_from_slice(escape_unprintable(progname).as_bytes());
    m.extend_from_slice(b" [options] [terminal]\n");
    m.extend_from_slice(USAGE);
    ulclosestream::stderr_write(&m);
    Exit(1)
}

/// `print_shell_commands (ttype)`: `TERM=TYPE;`, or the C shell's three
/// lines when `SHELL` ends in `csh`.
fn print_shell_commands(out: &mut ulclosestream::Stdout, ttype: &[u8]) {
    let csh = std::env::var_os("SHELL").is_some_and(|shell| {
        let shell = os_bytes(&shell);
        let leaf = nc::rootname(&shell);
        leaf.len() >= 3 && leaf.ends_with(b"csh")
    });
    let mut m = Vec::new();
    if csh {
        m.extend_from_slice(b"set noglob;\nsetenv TERM ");
        m.extend_from_slice(ttype);
        m.extend_from_slice(b";\nunset noglob;\n");
    } else {
        m.extend_from_slice(b"TERM=");
        m.extend_from_slice(ttype);
        m.extend_from_slice(b";\n");
    }
    out.write(&m);
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

/// The options, as `main`'s `getopt` loop leaves them.
#[derive(Default)]
struct Options {
    noinit: bool,
    noset: bool,
    quiet: bool,
    sflag_obsolete: bool,
    sflag: bool,
    showterm: bool,
    opt_c: bool,
    opt_w: bool,
    terasechar: i32,
    intrchar: i32,
    tkillchar: i32,
    /// `-a`, `-d`, `-m`, `-p`, in order.
    maps: Vec<Map>,
    operands: Vec<Vec<u8>>,
}

/// Upstream's `main`, up to its `ExitProgram`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> Result<u8, Exit> {
    let mut words: Vec<Vec<u8>> = argv.iter().map(|a| os_bytes(a).into_owned()).collect();
    let progname = nc::rootname(words.first().map(Vec::as_slice).unwrap_or_default()).to_vec();
    obsolete(&mut words);
    let argv0 = words.first().cloned().unwrap_or_default();
    let rewritten: Vec<OsString> = words
        .iter()
        .map(|w| coreutils::quote::os_from_bytes(w))
        .collect();

    let mut o = Options {
        terasechar: -1,
        intrchar: -1,
        tkillchar: -1,
        ..Options::default()
    };
    for item in TSET
        .parse(
            rewritten.get(1..).unwrap_or_default(),
            "a:cd:e:Ii:k:m:p:qQrSsVw",
            &[],
        )
        .short_only(true)
    {
        let value = |v: Option<OsString>| v.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
        // The obsolete three name the port the mapping is for.
        let mut map = |port: Option<&[u8]>, v: Option<OsString>| -> Result<(), Exit> {
            o.maps.push(add_mapping(&progname, port, &value(v))?);
            Ok(())
        };
        match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return Err(usage(&progname));
            }
            Ok(Opt::Short(b'a', v)) => map(Some(b"arpanet".as_slice()), v)?,
            Ok(Opt::Short(b'd', v)) => map(Some(b"dialup".as_slice()), v)?,
            Ok(Opt::Short(b'm', v)) => map(None, v)?,
            Ok(Opt::Short(b'p', v)) => map(Some(b"plugboard".as_slice()), v)?,
            Ok(Opt::Short(b'c', _)) => o.opt_c = true,
            Ok(Opt::Short(b'e', v)) => o.terasechar = arg_to_char(&value(v)),
            Ok(Opt::Short(b'I', _)) => o.noinit = true,
            Ok(Opt::Short(b'i', v)) => o.intrchar = arg_to_char(&value(v)),
            Ok(Opt::Short(b'k', v)) => o.tkillchar = arg_to_char(&value(v)),
            Ok(Opt::Short(b'Q', _)) => o.quiet = true,
            Ok(Opt::Short(b'q', _)) => o.noset = true,
            Ok(Opt::Short(b'r', _)) => o.showterm = true,
            Ok(Opt::Short(b'S', _)) => o.sflag_obsolete = true,
            Ok(Opt::Short(b's', _)) => o.sflag = true,
            Ok(Opt::Short(b'V', _)) => {
                out.write(&nc::version_line(&progname));
                return Ok(0);
            }
            Ok(Opt::Short(b'w', _)) => o.opt_w = true,
            Ok(Opt::Operand(w)) => o.operands.push(os_bytes(w).into_owned()),
            Ok(Opt::Short(..) | Opt::Long(..)) => return Err(usage(&progname)),
        }
    }
    if o.operands.len() > 1 {
        return Err(usage(&progname));
    }
    if !o.opt_c && !o.opt_w {
        o.opt_c = true;
        o.opt_w = true;
    }

    let (tty, saved) = TtySettings::save(true, &progname).map_err(Exit)?;
    let mut mode = saved.unwrap_or_default();
    let mut oldmode = mode;
    let ospeed = libcall::termios::output_speed(&mode);
    let mut tset = Tset {
        progname,
        tty,
        maps: std::mem::take(&mut o.maps),
        ospeed,
        stdin: StdioReader::stdin(),
    };
    let outcome = sequence(&mut tset, &o, out, &mut mode, &mut oldmode);
    // What `exit` does to a stream that was read.
    tset.stdin.exit_sync();
    outcome
}

/// The rest of `main`: the type found, and the terminal set up.
fn sequence(
    tset: &mut Tset,
    o: &Options,
    out: &mut ulclosestream::Stdout,
    mode: &mut Termios,
    oldmode: &mut Termios,
) -> Result<u8, Exit> {
    let my_fd = tset.tty.fd();
    let use_reset = nc::same_program(&tset.progname, b"reset");
    if use_reset {
        nc::reset_tty_settings(my_fd, mode, o.noset);
    }

    let (ttype, entry, padding) = get_termcap_entry(tset, my_fd, o.operands.first().cloned())?;
    let mut legacy = Legacy::of(&entry);
    let strings = terminfo::legacy_copy(&entry, &terminfo::Env::from_process());
    let mut tparm = Tparm::new();

    if !o.noset {
        if o.opt_w {
            nc::set_window_size(my_fd, &mut legacy);
        }
        if o.opt_c {
            nc::set_control_chars(mode, &strings, o.terasechar, o.intrchar, o.tkillchar);
            nc::set_conversions(mode, &strings);
            if !o.noinit {
                let mut file = MyFile::Stderr;
                let mut reset = Reset {
                    file: &mut file,
                    use_reset,
                    use_init: !use_reset,
                    entry: &entry,
                    strings: &strings,
                    legacy: &mut legacy,
                    tparm: &mut tparm,
                    padding,
                    progname: &tset.progname,
                };
                if reset
                    .send_init_strings(my_fd, Some(oldmode), &tset.tty)
                    .map_err(Exit)?
                {
                    ulclosestream::stderr_write(b"\r");
                    // "Settle the terminal."
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            tset.tty.update(oldmode, mode);
        }
    }

    if o.noset {
        let mut line = ttype.clone();
        line.push(b'\n');
        out.write(&line);
    } else {
        if o.showterm {
            let mut m = b"Terminal type is ".to_vec();
            m.extend_from_slice(&ttype);
            m.extend_from_slice(b".\n");
            ulclosestream::stderr_write(&m);
        }
        // "If erase, kill and interrupt characters could have been modified
        // and not -Q, display the changes."
        if !o.quiet {
            nc::print_tty_chars(oldmode, mode, &strings);
        }
    }

    if o.sflag_obsolete {
        return Err(tset.err(b"The -S option is not supported under terminfo."));
    }
    if o.sflag {
        print_shell_commands(out, &ttype);
    }
    Ok(0)
}

/// `get_termcap_entry (fd, userarg)`: the type -- the operand as it is, or
/// `TERM` (or `unknown`) mapped -- asked for when it starts with `?`, and
/// asked for again until `setupterm` takes it. `TERMCAP` leaves the
/// environment unless it is a path.
fn get_termcap_entry(
    tset: &mut Tset,
    fd: i32,
    userarg: Option<Vec<u8>>,
) -> Result<(Vec<u8>, Entry, terminfo::Padding), Exit> {
    let mut ttype = match userarg {
        Some(t) => t,
        None => {
            let term = std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned());
            // No `/etc/ttys` lookup: the reference's build has none.
            tset.mapped(term.unwrap_or_else(|| b"unknown".to_vec()))
        }
    };

    // "If not a path, remove TERMCAP from the environment so we get a real
    // entry from /etc/termcap."
    if std::env::var_os("TERMCAP").is_some_and(|t| os_bytes(&t).first() != Some(&b'/')) {
        // SAFETY: the program has one thread, so nothing reads the
        // environment while it changes. What is left of it reaches only an
        // `iprog` child.
        unsafe { std::env::remove_var("TERMCAP") };
    }

    if ttype.first() == Some(&b'?') {
        let rest = ttype.get(1..).unwrap_or_default().to_vec();
        ttype = tset.askuser((!rest.is_empty()).then_some(rest.as_slice()))?;
    }

    let env = terminfo::Env::from_process();
    let opts = terminfo::Options {
        fd,
        ..terminfo::Options::default()
    };
    loop {
        let setup = terminfo::setupterm_with(Some(&ttype), None, &env, &opts);
        if setup.complaint.is_none()
            && let Some(entry) = setup.entry.clone()
        {
            return Ok((ttype, entry, setup.padding()));
        }
        let mut m = escape_unprintable(&tset.progname).into_bytes();
        if setup.status == 0 {
            m.extend_from_slice(b": unknown terminal type ");
            m.extend_from_slice(escape_unprintable(&ttype).as_bytes());
            m.push(b'\n');
        } else {
            m.extend_from_slice(b": can't initialize terminal type ");
            m.extend_from_slice(escape_unprintable(&ttype).as_bytes());
            m.extend_from_slice(format!(" (error {})\n", setup.status).as_bytes());
        }
        ulclosestream::stderr_write(&m);
        ttype = tset.askuser(None)?;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn words(w: &[&str]) -> Vec<Vec<u8>> {
        w.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn the_obsolete_forms_are_rewritten_where_no_argument_follows() {
        let mut argv = words(&["tset", "-", "-e", "-i", "x", "-k"]);
        obsolete(&mut argv);
        assert_eq!(argv, words(&["tset", "-q", "-e^H", "-i", "x", "-k^U"]));
        let mut argv = words(&["tset", "-e", ""]);
        obsolete(&mut argv);
        assert_eq!(argv, words(&["tset", "-e", ""]));
    }

    #[test]
    fn a_character_is_a_signed_char() {
        assert_eq!(arg_to_char(b"^H"), 8);
        assert_eq!(arg_to_char(b"^h"), 8);
        assert_eq!(arg_to_char(b"^?"), 0o177);
        assert_eq!(arg_to_char(b"^"), i32::from(b'^'));
        assert_eq!(arg_to_char(b"x"), i32::from(b'x'));
        assert_eq!(arg_to_char(b""), 0);
        assert_eq!(arg_to_char(b"\xff"), -1);
    }

    #[test]
    fn the_baud_rate_table_stops_where_the_speeds_do() {
        assert_eq!(tbaudrate(b"tset", b"110").ok(), Some(3));
        assert_eq!(tbaudrate(b"tset", b"B134").ok(), Some(4));
        assert!(tbaudrate(b"tset", b"134.5").is_err());
        assert!(tbaudrate(b"tset", b"9600").is_err());
    }

    #[test]
    fn a_mapping_is_read_as_upstream_reads_it() {
        let m = add_mapping(b"tset", None, b"xterm>110:vt100").ok().unwrap();
        assert_eq!(m.porttype.as_deref(), Some(&b"xterm"[..]));
        assert_eq!((m.conditional, m.speed), (GT, 3));
        assert_eq!(m.ttype, b"vt100");
        let m = add_mapping(b"tset", None, b"vt100").ok().unwrap();
        assert_eq!((m.porttype, m.ttype), (None, b"vt100".to_vec()));
        // `!` alone inverts no test into all three.
        let m = add_mapping(b"tset", None, b"!110:x").ok().unwrap();
        assert_eq!(m.conditional, EQ | GT | LT);
        let m = add_mapping(b"tset", Some(b"dialup"), b":x").ok().unwrap();
        assert_eq!(m.porttype.as_deref(), Some(&b"dialup"[..]));
        for bad in [&b"@:x"[..], b"<>110:x", b">110"] {
            assert!(add_mapping(b"tset", None, bad).is_err());
        }
        // No test character at all: the whole is a type, digits or not.
        let m = add_mapping(b"tset", None, b"110").ok().unwrap();
        assert_eq!((m.porttype, m.ttype), (None, b"110".to_vec()));
        assert!(add_mapping(b"tset", Some(b"dialup"), b"x:y").is_err());
    }
}
