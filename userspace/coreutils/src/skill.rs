//! procps-ng 4.0.4's `skill` and `snice`: signal, or set the nice value of,
//! the processes an expression picks -- by terminal, user, process id or
//! command name. One program, `src/skill.c`, which is one or the other by
//! the name it was started under; `bin/skill.rs` and `bin/snice.rs` both run
//! [`main`], and Ubuntu ships `snice` as a link to `skill`.
//!
//! A transcription. What upstream does and this keeps:
//!
//! - The signal (`-9`, `-KILL`, `-SIGKILL`) or the priority (`+5`, `-5`) is
//!   taken out of the arguments before `getopt` sees them: for `skill` the
//!   first word that is `-` and a signal name or number, for `snice` every
//!   word that is a sign and a digit, the last one winning.
//! - A word that is not an option is a process id if `strtol` reads all of it,
//!   and a command name if not. `-t` names a terminal under `/dev` and `-u` a
//!   user; one that is not there is passed over without a word.
//! - `--ns PID` reads that process's namespaces but compares them only when
//!   `--nslist` named which, so alone it adds no condition at all -- and
//!   counts as one, so `skill --ns PID` picks every process. `--nslist`
//!   takes any name, one that is no namespace included (see
//!   `parse_namespaces`).
//! - `-n` prints the id of each process picked and signals none -- each is
//!   sent signal 0 -- and `-v` (or `-d`, or `-w` and a failure) prints a line
//!   for each on standard error and not the id. `-i` asks first, on standard
//!   error, and takes an answer starting with `y` or `Y` as yes.
//! - The status is 0 once the processes have been looked at, whatever became
//!   of them, and nothing is said about standard output that could not be
//!   written: `skill.c` registers no `close_stdout`.
//! - The program first tries to raise its own priority to -20. An empty `-p`
//!   argument is reported with the reason that left -- `Permission denied`
//!   for anyone but root -- since upstream's `strtol_or_err` reports it with
//!   whatever `errno` it finds.
//!
//! # Deliberately different
//!
//! - `-V`/`--version` names this build.
//! - An argument echoed back in a diagnostic between apostrophes goes through
//!   `quoteaf` (`free`'s divergence 6).

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::getopt::{Opt, Program as Parser, Takes};
use crate::procps::devname::{ABBREV_DEV, Devname};
use crate::procps::namespace::{NS_NAMES, ns_get_id, ns_read_pid};
use crate::procps::pwcache::Pwcache;
use crate::procps::readproc::{Fill, Proc, Reader};
use crate::procps::signals;
use crate::procps::strutils::{self, NumFault};
use crate::quote::{os_bytes, os_from_bytes, quoteaf};
use crate::stdfd::{self, Stream};

/// `DEFAULT_NICE`.
const DEFAULT_NICE: i32 = 4;
/// `SIGTERM`, the signal when none is named.
const SIGTERM: i32 = 15;

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`.
const SKILL: Parser = Parser::new("skill", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "c:dfilnp:Lt:u:vwhV";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("command", Takes::Required),
    ("debug", Takes::Nothing),
    ("fast", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("list", Takes::Nothing),
    ("no-action", Takes::Nothing),
    ("pid", Takes::Required),
    ("table", Takes::Nothing),
    ("tty", Takes::Required),
    ("user", Takes::Required),
    ("ns", Takes::Required),
    ("nslist", Takes::Required),
    ("verbose", Takes::Nothing),
    ("warnings", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Which program this is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Skill,
    Snice,
}

/// `struct run_time_conf_t`.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct RunTime {
    fast: bool,
    interactive: bool,
    verbose: bool,
    warnings: bool,
    noaction: bool,
    debugging: bool,
}

/// Everything upstream keeps in globals.
struct Skill {
    /// `program_invocation_short_name`.
    name: Vec<u8>,
    which: Which,
    rt: RunTime,
    ttys: Vec<i32>,
    uids: Vec<u32>,
    cmds: Vec<Vec<u8>>,
    pids: Vec<i32>,
    /// `namespaces`: the names `--nslist` gave, which are what makes the
    /// namespaces count.
    namespaces: Vec<Vec<u8>>,
    ns_pid: i32,
    match_namespaces: [u64; 8],
    ns_flags: u32,
    my_pid: i32,
    sig_or_pri: i32,
    /// The `errno` the attempt to raise this process's priority left.
    stale_errno: i32,
    out: Stream,
    root: PathBuf,
    rtmin: i32,
    utf8: bool,
}

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// `strerror (errno)`.
fn strerror(errno: i32) -> String {
    crate::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// `printf ("%-*s")` and `("%-*.*s")`: `s`, cut to `cut` bytes if given,
/// padded to `width`.
fn left(s: &[u8], width: usize, cut: Option<usize>) -> Vec<u8> {
    let s = cut.map_or(s, |c| s.get(..c.min(s.len())).unwrap_or(s));
    let mut v = s.to_vec();
    v.resize(width.max(s.len()), b' ');
    v
}

/// `atoi`: `strtol`'s value at base 10, cut to `int`, 0 when there is none.
fn atoi(s: &[u8]) -> i32 {
    crate::procps::scanf::atoi(s)
}

/// The usage text, for the program this is.
fn usage_text(which: Which, name: &[u8]) -> Vec<u8> {
    let mut t = b"\nUsage:\n ".to_vec();
    t.extend_from_slice(name);
    t.extend_from_slice(match which {
        Which::Skill => b" [signal] [options] <expression>\n",
        Which::Snice => b" [new priority] [options] <expression>\n",
    });
    t.extend_from_slice(
        concat!(
            "\n",
            "Options:\n",
            " -f, --fast         fast mode (not implemented)\n",
            " -i, --interactive  interactive\n",
            " -l, --list         list all signal names\n",
            " -L, --table        list all signal names in a nice table\n",
            " -n, --no-action    do not actually kill processes; just print what would happen\n",
            " -v, --verbose      explain what is being done\n",
            " -w, --warnings     enable warnings (not implemented)\n",
            "\n",
            "Expression can be: terminal, user, pid, command.\n",
            "The options below may be used to ensure correct interpretation.\n",
            " -c, --command <command>  expression is a command name\n",
            " -p, --pid <pid>          expression is a process id number\n",
            " -t, --tty <tty>          expression is a terminal\n",
            " -u, --user <username>    expression is a username\n",
            "\n",
            "Alternatively, expression can be:\n",
            " --ns <pid>               match the processes that belong to the same\n",
            "                          namespace as <pid>\n",
            " --nslist <ns,...>        list which namespaces will be considered for\n",
            "                          the --ns option; available namespaces are\n:",
            "                          ipc, mnt, net, pid, user, uts\n",
            "\n",
            "\n",
            " -h, --help     display this help and exit\n",
            " -V, --version  output version information and exit\n",
        )
        .as_bytes(),
    );
    t.extend_from_slice(match which {
        Which::Skill => concat!(
            "\n",
            "The default signal is TERM. Use -l or -L to list available signals.\n",
            "Particularly useful signals include HUP, INT, KILL, STOP, CONT, and 0.\n",
            "Alternate signals may be specified in three ways: -SIGKILL -KILL -9\n",
            "\n",
            "For more details see skill(1).\n",
        )
        .as_bytes(),
        Which::Snice => concat!(
            "\n",
            "The default priority is +4. (snice +4 ...)\n",
            "Priority numbers range from +20 (slowest) to -20 (fastest).\n",
            "Negative priority numbers are restricted to administrative users.\n",
            "\n",
            "For more details see snice(1).\n",
        )
        .as_bytes(),
    });
    t
}

impl Skill {
    /// Bytes to standard output, kept for the end as stdio keeps them.
    /// `Stream` never fails a write; what becomes of it is not looked at,
    /// as upstream does not look.
    fn put(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    /// `xwarnx`: glibc's `error (0, 0, …)` -- standard output delivered
    /// first, then `NAME: MESSAGE`.
    fn warnx(&self, msg: &[u8]) {
        let mut m = self.name.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(msg);
        m.push(b'\n');
        stdfd::diag_bytes(&m);
    }

    /// `xerrx (EXIT_FAILURE, …)`.
    fn errx(&self, msg: &[u8]) -> u8 {
        self.warnx(msg);
        1
    }

    /// `skillsnice_usage (out)`, and the status `exit` is then given.
    fn usage(&mut self, to_stdout: bool) -> u8 {
        let text = usage_text(self.which, &self.name);
        if to_stdout {
            self.put(&text);
            0
        } else {
            stdfd::diag_bytes_ahead_of_stdout(&text);
            1
        }
    }

    /// `strtol_or_err (arg, "failed to parse argument")`.
    fn strtol_or_err(&self, arg: &[u8]) -> Result<i64, u8> {
        strutils::strtol(arg).map_err(|fault| {
            let mut m = b"failed to parse argument: ".to_vec();
            m.extend_from_slice(quoteaf(arg).as_bytes());
            match fault {
                // An empty argument is never converted, and is reported with
                // whatever `errno` was left over.
                NumFault::NoConversion if arg.is_empty() && self.stale_errno != 0 => {
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(strerror(self.stale_errno).as_bytes());
                }
                _ => m.extend_from_slice(fault.suffix().as_bytes()),
            }
            self.errx(&m)
        })
    }

    /// `snice_prio_option`: every word that is a sign and a digit taken out
    /// of `argv`, the last of them the priority.
    fn snice_prio_option(&self, argv: &mut Vec<Vec<u8>>) -> Result<i32, u8> {
        let mut prio = i64::from(DEFAULT_NICE);
        let mut i = 1;
        while i < argv.len() {
            let word = argv.get(i).cloned().unwrap_or_default();
            let signed = matches!(word.first(), Some(b'-' | b'+'));
            if signed && word.get(1).is_some_and(u8::is_ascii_digit) {
                prio = self.strtol_or_err(&word)?;
                if i32::try_from(prio).is_err() {
                    // `%lu` of the `long`.
                    let m = format!("priority {} out of range", prio.cast_unsigned());
                    return Err(self.errx(m.as_bytes()));
                }
                argv.remove(i);
            } else {
                i = i.saturating_add(1);
            }
        }
        Ok(i32::try_from(prio).unwrap_or(DEFAULT_NICE))
    }

    /// `parse_namespaces`: `--nslist`'s names, each a bit of `ns_flags` and
    /// an entry in the list.
    ///
    /// Upstream refuses a name that is not a namespace -- "is not a valid
    /// namespace", then "invalid namespace list" and the usage -- when
    /// `procps_ns_get_id` answers -1. It never does: it answers `-EINVAL`. So
    /// every name is taken. One that is not a namespace sets the bit
    /// `1 << -22`, which x86's shift, counting modulo 32, makes bit 10 -- no
    /// namespace's -- and still puts a name in the list, which is what makes
    /// the namespaces count at all.
    fn parse_namespaces(&mut self, arg: &[u8]) {
        /// `-EINVAL`, `procps_ns_get_id`'s answer for a name that is none.
        const NOT_A_NAMESPACE: i32 = -22;
        self.ns_flags = 0;
        for name in arg.split(|&c| c == b',') {
            let id = ns_get_id(name).map_or(NOT_A_NAMESPACE, |i| {
                i32::try_from(i).unwrap_or(NOT_A_NAMESPACE)
            });
            self.ns_flags |= 1u32.wrapping_shl(id.cast_unsigned());
            self.namespaces.push(name.to_vec());
        }
    }

    /// `parse_options`. `Err` is the status to exit with, everything said.
    #[allow(clippy::too_many_lines)]
    fn parse_options(&mut self, argv: &mut Vec<Vec<u8>>) -> Result<(), u8> {
        if argv.len() < 2 {
            return Err(self.usage(false));
        }
        self.sig_or_pri = -1;
        let mut prino = DEFAULT_NICE;
        match self.which {
            Which::Snice => prino = self.snice_prio_option(argv)?,
            Which::Skill => {
                let signo = signals::skill_sig_option(argv, self.rtmin);
                if signo > -1 {
                    self.sig_or_pri = signo;
                }
            }
        }

        let argv0 = argv.first().cloned().unwrap_or_default();
        let words: Vec<OsString> = argv.iter().skip(1).map(|w| os_from_bytes(w)).collect();
        let mut operands: Vec<Vec<u8>> = Vec::new();
        for item in SKILL.parse(&words, SHORT_OPTIONS, LONG_OPTIONS) {
            match item {
                Ok(Opt::Short(b'c', Some(v)) | Opt::Long("command", Some(v))) => {
                    self.cmds.push(os_bytes(&v).into_owned());
                }
                Ok(Opt::Short(b'd', _) | Opt::Long("debug", _)) => self.rt.debugging = true,
                Ok(Opt::Short(b'f', _) | Opt::Long("fast", _)) => self.rt.fast = true,
                Ok(Opt::Short(b'i', _) | Opt::Long("interactive", _)) => self.rt.interactive = true,
                Ok(Opt::Short(b'l', _) | Opt::Long("list", _)) => {
                    let text = signals::unix_print_signals(self.rtmin);
                    self.put(&text);
                    return Err(0);
                }
                Ok(Opt::Short(b'n', _) | Opt::Long("no-action", _)) => self.rt.noaction = true,
                Ok(Opt::Short(b'p', Some(v)) | Opt::Long("pid", Some(v))) => {
                    let n = self.strtol_or_err(&os_bytes(&v))?;
                    // An `int` list: the `long` cut to its low 32 bits.
                    self.pids.push(crate::procps::scanf::low_i32(n));
                }
                Ok(Opt::Short(b'L', _) | Opt::Long("table", _)) => {
                    let text = signals::pretty_print_signals(self.rtmin);
                    self.put(&text);
                    return Err(0);
                }
                Ok(Opt::Short(b't', Some(v)) | Opt::Long("tty", Some(v))) => {
                    // `snprintf (path, 32, "/dev/%s", …)`: 31 bytes at most.
                    let mut path = b"/dev/".to_vec();
                    path.extend_from_slice(&os_bytes(&v));
                    path.truncate(31);
                    if let Some(rdev) = char_device(&path) {
                        self.ttys.push(rdev);
                    }
                }
                Ok(Opt::Short(b'u', Some(v)) | Opt::Long("user", Some(v))) => {
                    let pw = Pwcache::load();
                    if let Some(uid) = pw.uid_by_name(&os_bytes(&v)) {
                        self.uids.push(uid);
                    }
                }
                Ok(Opt::Long("ns", Some(v))) => {
                    let arg = os_bytes(&v).into_owned();
                    self.ns_pid = atoi(&arg);
                    if self.ns_pid == 0 {
                        let mut m = b"invalid pid number ".to_vec();
                        m.extend_from_slice(&arg);
                        self.warnx(&m);
                        return Err(self.usage(false));
                    }
                    match ns_read_pid(&self.root, self.ns_pid) {
                        Some(ns) => self.match_namespaces = ns,
                        None => {
                            self.warnx(b"error reading reference namespace information");
                            return Err(self.usage(false));
                        }
                    }
                }
                Ok(Opt::Long("nslist", Some(v))) => self.parse_namespaces(&os_bytes(&v)),
                Ok(Opt::Short(b'v', _) | Opt::Long("verbose", _)) => self.rt.verbose = true,
                Ok(Opt::Short(b'w', _) | Opt::Long("warnings", _)) => self.rt.warnings = true,
                Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return Err(self.usage(true)),
                Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                    let mut v = self.name.clone();
                    v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                    self.put(&v);
                    return Err(0);
                }
                Ok(Opt::Operand(o)) => operands.push(os_bytes(o).into_owned()),
                // Nothing else is in the tables.
                Ok(Opt::Short(..) | Opt::Long(..)) => return Err(self.usage(false)),
                Err(e) => {
                    let mut m = argv0.clone();
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(e.sentence.as_bytes());
                    m.push(b'\n');
                    stdfd::diag_bytes_ahead_of_stdout(&m);
                    return Err(self.usage(false));
                }
            }
        }

        for word in operands {
            let (num, used, overflow) = cstrtol::strtol_overflow(&word, 10);
            if !overflow && used != 0 && used == word.len() {
                self.pids.push(crate::procps::scanf::low_i32(num));
            } else {
                self.cmds.push(word);
            }
        }

        if self.ttys.is_empty()
            && self.uids.is_empty()
            && self.cmds.is_empty()
            && self.pids.is_empty()
            && self.ns_pid == 0
        {
            return Err(self.errx(b"no process selection criteria"));
        }
        let rt = &self.rt;
        if rt.interactive && (rt.verbose || rt.fast || rt.noaction) {
            return Err(self.errx(b"-i makes no sense with -v, -f, and -n"));
        }
        if rt.verbose && (rt.interactive || rt.fast) {
            return Err(self.errx(b"-v makes no sense with -i and -f"));
        }
        if self.rt.noaction {
            self.which = Which::Skill;
            // Signal 0: harmless.
            self.sig_or_pri = 0;
        }
        if self.which == Which::Snice {
            self.sig_or_pri = prino;
        } else if self.sig_or_pri < 0 {
            self.sig_or_pri = SIGTERM;
        }
        Ok(())
    }

    /// `show_lists`: `-d`'s account of what was asked for, last first.
    fn show_lists(&self) {
        let mut m = format!("signal: {}\n", self.sig_or_pri).into_bytes();
        m.extend(list(
            "TTY",
            self.ttys
                .iter()
                .map(|t| format!("{},{}", (t >> 8) & 0xff, t & 0xff).into_bytes()),
        ));
        m.extend(list(
            "UID",
            self.uids
                .iter()
                .map(|u| u.cast_signed().to_string().into_bytes()),
        ));
        m.extend(list(
            "PID",
            self.pids.iter().map(|p| p.to_string().into_bytes()),
        ));
        m.extend(list("CMD", self.cmds.iter().cloned()));
        stdfd::diag_bytes_ahead_of_stdout(&m);
    }

    /// `ask_user`: the process described and `?` on standard error, and a
    /// line read; yes if it starts with `y` or `Y`.
    fn ask_user(&mut self, p: &Proc, tty: &[u8]) -> bool {
        let mut m = describe(p, tty);
        m.extend_from_slice(b"? ");
        stdfd::diag_bytes_ahead_of_stdout(&m);
        // `fflush (stdout)`: what has been printed goes out before the wait.
        let _ = self.out.flush();
        let mut answer = Vec::new();
        match std::io::stdin().lock().read_until(b'\n', &mut answer) {
            Ok(0) | Err(_) => false,
            Ok(_) => matches!(answer.first(), Some(b'y' | b'Y')),
        }
    }

    /// `nice_or_kill`: the signal sent, or the priority set, and what is
    /// said of it.
    fn nice_or_kill(&mut self, p: &Proc, tty: &[u8]) {
        if self.rt.interactive && !self.ask_user(p, tty) {
            return;
        }
        let result = match self.which {
            Which::Skill => libcall::kill(p.tid, self.sig_or_pri),
            Which::Snice => libcall::priority::setpriority(
                libcall::priority::PRIO_PROCESS,
                p.tid.cast_unsigned(),
                self.sig_or_pri,
            ),
        };
        let failed = result.is_err();
        if (self.rt.warnings && failed) || self.rt.debugging || self.rt.verbose {
            let mut m = describe(p, tty);
            // `perror ("")`: the reason alone -- `Success` when there was
            // none.
            m.extend_from_slice(strerror(result.err().unwrap_or(0)).as_bytes());
            m.push(b'\n');
            stdfd::diag_bytes_ahead_of_stdout(&m);
            return;
        }
        if self.rt.interactive {
            return;
        }
        if self.rt.noaction {
            let line = format!("{}\n", p.tid);
            self.put(line.as_bytes());
        }
    }

    /// `scan_procs`: every process but this one and 0, against every list.
    fn scan_procs(&mut self) -> Result<(), u8> {
        let fill = Fill {
            stat: true,
            usr: true,
            ..Fill::default()
        };
        let mut reader = Reader::new(self.root.clone(), fill, self.utf8, Pwcache::load());
        let Ok(procs) = reader.reap() else {
            return Err(self.errx(b"Unable to load process information"));
        };
        let mut devname = Devname::new(self.root.clone());
        for p in &procs {
            if p.tid == self.my_pid || p.tid == 0 {
                continue;
            }
            if !self.pids.is_empty() && !self.pids.contains(&p.tid) {
                continue;
            }
            if !self.uids.is_empty() && !self.uids.contains(&p.euid) {
                continue;
            }
            if !self.ttys.is_empty() && !self.ttys.contains(&p.tty) {
                continue;
            }
            let cmd = p.cmd.as_deref().unwrap_or_default();
            if !self.cmds.is_empty() && !self.cmds.iter().any(|c| c == cmd) {
                continue;
            }
            if !self.namespaces.is_empty() && !self.match_ns(p.tid)? {
                continue;
            }
            let tty = devname.dev_to_tty(64, p.tty.cast_unsigned(), p.tid, ABBREV_DEV);
            self.nice_or_kill(p, &tty);
        }
        Ok(())
    }

    /// `match_ns`: whether the process shares every namespace `--nslist`
    /// named with `--ns`'s.
    fn match_ns(&self, pid: i32) -> Result<bool, u8> {
        let Some(ns) = ns_read_pid(&self.root, pid) else {
            return Err(self.errx(b"Unable to read process namespace information"));
        };
        Ok((0..NS_NAMES.len()).all(|i| {
            let bit = 1u32
                .checked_shl(u32::try_from(i).unwrap_or(31))
                .unwrap_or(0);
            self.ns_flags & bit == 0 || ns.get(i) == self.match_namespaces.get(i)
        }))
    }
}

/// `"%-8s %-8s %5d %-16.16s   "`: terminal, user, process and command.
fn describe(p: &Proc, tty: &[u8]) -> Vec<u8> {
    let mut m = left(tty, 8, None);
    m.push(b' ');
    m.extend(left(p.euser.as_deref().unwrap_or_default(), 8, None));
    m.push(b' ');
    m.extend(format!("{:>5} ", p.tid).into_bytes());
    m.extend(left(p.cmd.as_deref().unwrap_or_default(), 16, Some(16)));
    m.extend_from_slice(b"   ");
    m
}

/// One of `show_lists`' lines: the count, the name, the items last first.
fn list(
    name: &str,
    items: impl DoubleEndedIterator<Item = Vec<u8>> + ExactSizeIterator,
) -> Vec<u8> {
    let mut m = format!("{} {name}: ", items.len()).into_bytes();
    let all: Vec<Vec<u8>> = items.rev().collect();
    if all.is_empty() {
        m.push(b'\n');
        return m;
    }
    let last = all.len().saturating_sub(1);
    for (i, item) in all.into_iter().enumerate() {
        m.extend_from_slice(&item);
        m.push(if i == last { b'\n' } else { b' ' });
    }
    m
}

/// `stat (path)` of a character device: its `st_rdev`, as the `int` the list
/// keeps.
#[cfg(unix)]
fn char_device(path: &[u8]) -> Option<i32> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = std::fs::metadata(std::path::Path::new(&os_from_bytes(path))).ok()?;
    if !meta.file_type().is_char_device() {
        return None;
    }
    Some(crate::procps::scanf::low_i32(i64::from_le_bytes(
        meta.rdev().to_le_bytes(),
    )))
}

/// The host build has no devices; it never reads a real `/proc`.
#[cfg(not(unix))]
fn char_device(_path: &[u8]) -> Option<i32> {
    None
}

/// The program, as `bin/skill.rs` and `bin/snice.rs` run it.
#[must_use]
pub fn main() -> ExitCode {
    stdfd::restore();
    let mut argv: Vec<Vec<u8>> = std::env::args_os()
        .map(|a| os_bytes(&a).into_owned())
        .collect();
    let argv0 = argv.first().cloned().unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    let which = match name.as_slice() {
        b"skill" | b"lt-skill" => Which::Skill,
        b"snice" | b"lt-snice" => Which::Snice,
        _ => {
            let mut m = b"skill: \"".to_vec();
            m.extend_from_slice(&name);
            m.extend_from_slice(b"\" is not supported\n");
            stdfd::diag_bytes_ahead_of_stdout(&m);
            stdfd::diag_bytes_ahead_of_stdout(b"\nFor more details see skill(1).\n");
            return ExitCode::from(1);
        }
    };
    let my_pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    // `setpriority (PRIO_PROCESS, my_pid, -20)`, its answer unlooked-at but
    // for the `errno` it leaves.
    let stale_errno = libcall::priority::setpriority(
        libcall::priority::PRIO_PROCESS,
        my_pid.cast_unsigned(),
        -20,
    )
    .err()
    .unwrap_or(0);
    let mut skill = Skill {
        name,
        which,
        rt: RunTime::default(),
        ttys: Vec::new(),
        uids: Vec::new(),
        cmds: Vec::new(),
        pids: Vec::new(),
        namespaces: Vec::new(),
        ns_pid: 0,
        match_namespaces: [0; 8],
        ns_flags: 0x3f,
        my_pid,
        sig_or_pri: -1,
        stale_errno,
        out: Stream::stdout(),
        root: PathBuf::from("/proc"),
        rtmin: libcall::sigrtmin(),
        utf8: crate::locale::ctype_is_utf8(),
    };
    let status = match skill.parse_options(&mut argv) {
        Err(status) => status,
        Ok(()) => {
            if skill.rt.debugging {
                skill.show_lists();
            }
            match skill.scan_procs() {
                Ok(()) => 0,
                Err(status) => status,
            }
        }
    };
    // No `close_stdout`: upstream registers none, so output that cannot be
    // delivered is lost without a word, and the status stands.
    let _ = skill.out.finish();
    ExitCode::from(status)
}

#[cfg(test)]
mod tests {
    use super::{Which, left, list, usage_text};

    #[test]
    fn columns_are_printf_widths() {
        assert_eq!(left(b"pts/0", 8, None), b"pts/0   ");
        assert_eq!(left(b"a-rather-long-name", 8, None), b"a-rather-long-name");
        assert_eq!(
            left(b"0123456789abcdefXYZ", 16, Some(16)),
            b"0123456789abcdef"
        );
    }

    #[test]
    fn show_lists_prints_the_last_first() {
        let l = list(
            "PID",
            [b"1".to_vec(), b"2".to_vec(), b"3".to_vec()].into_iter(),
        );
        assert_eq!(l, b"3 PID: 3 2 1\n");
        let empty = list("CMD", Vec::<Vec<u8>>::new().into_iter());
        assert_eq!(empty, b"0 CMD: \n");
    }

    #[test]
    fn the_usage_names_the_program_it_is() {
        let skill = usage_text(Which::Skill, b"skill");
        assert!(skill.starts_with(b"\nUsage:\n skill [signal] [options] <expression>\n"));
        assert!(skill.ends_with(b"\nFor more details see skill(1).\n"));
        let snice = usage_text(Which::Snice, b"snice");
        assert!(snice.starts_with(b"\nUsage:\n snice [new priority] [options] <expression>\n"));
        assert!(snice.ends_with(b"\nFor more details see snice(1).\n"));
    }
}
