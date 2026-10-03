//! procps-ng 4.0.4's `pgrep`, `pkill` and `pidwait`: one program under three
//! names.
//!
//! Upstream builds `src/pgrep.c` three times, and the three executables differ
//! in nothing but their names. Each decides what it is from
//! `program_invocation_short_name` -- the part of `argv[0]` after its last
//! `/` -- so a `pgrep` started under the name `pkill` kills. This module is
//! that one program; `bin/pgrep.rs` and `bin/pkill.rs` are entry points to it
//! that differ in the same way, which is to say not at all.
//!
//! | started as | it | options only it takes |
//! |---|---|---|
//! | `pgrep` (or anything else) | prints the PIDs of the processes that match | `-d -l -a -v -w` |
//! | `pkill` | signals them | `-SIGNAL -q -e` |
//! | `pidwait` | waits until they have all exited | `-e` |
//!
//! Every *long* option is accepted under every name, because `getopt_long`
//! never consults the short-option string: `pkill --inverse` inverts, where
//! `pkill -v` is an invalid option.
//!
//! ## How a process is matched
//!
//! The process table is [`crate::procps::readproc`]'s, read as
//! `procps_pids_get` reads it -- one process at a time, in directory order,
//! the uptime read again for each. The selection options are tried in
//! upstream's order and the first that fails ends the test; then the pattern,
//! an extended regular expression in glibc's `REG_EXTENDED` dialect ([`ere`]),
//! is matched against the process name, or with `-f` its whole command line.
//! `-v` inverts the result, and `-n`/`-o` keep only the newest or oldest of
//! what is left.
//!
//! ## On SlateOS
//!
//! * `pkill` sends through the kernel's signal path, `kill(2)`, as the
//!   coreutils `kill` does. `-q` sends with `sigqueue`, which SlateOS's C
//!   library does not deliver yet; each such send reports `Function not
//!   implemented`.
//! * `pidwait` waits on `pidfd_open`, which the kernel implements for Linux
//!   programs but SlateOS's native system-call table has no number for
//!   (`known-issues/B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS.md`).
//!   Started as `pidwait`, this program says upstream's `pidfd_open() not
//!   implemented in Linux < 5.3` as soon as it has a process to wait for.
//!   So no `pidwait` is built: a command that cannot do its job is not
//!   shipped (design-decisions §1006). The day the call exists, the command
//!   is one file in `src/bin/` -- this module already is the whole of it, and
//!   `scripts/pgrep-diff.sh` already measures it.
//!
//! ## Deliberate differences
//!
//! 1. **`-V` names this build:** `pgrep from SlateOS coreutils 0.1.0`.
//! 2. **No PID below 1 is signalled.** Only a malformed `/proc` produces one
//!    -- a `status` without its `Pid:` line makes the PID 0, and a directory
//!    named past `int` wraps negative -- and upstream hands it to `kill`,
//!    where 0 means "my own process group" and -1 "every process I may
//!    signal". Here the send fails as `kill` reports a bad PID: `killing pid
//!    0 failed: Invalid argument`.
//! 3. **A backreference search that exceeds the regex engine's step budget
//!    ends the run** with `regex error: …` and status 3. glibc has no budget
//!    and would eventually answer; counting the abandoned search as "no
//!    match" would make `pkill --inverse` signal on an answer it never got.
//! 4. **An ancestor cycle ends `-A`'s walk.** Upstream follows parent PIDs
//!    until one is not found, which a `/proc` whose parents form a loop
//!    (no kernel writes one) turns into a loop that never ends.
//! 5. **Memory upstream never wrote reads as zero:** the name `pkill -e` and
//!    `pidwait -e` print for a process selected without a pattern, before
//!    any name was copied, is the empty string, which is what upstream's
//!    freshly mapped buffer holds.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use ere::{Regex, Syntax};

use crate::getopt::{Opt, Program, Takes};
use crate::procps::cvt::{cvt_i32, cvt_u64, dbl};
use crate::procps::devname::{ABBREV_DEV, Devname};
use crate::procps::pwcache::Pwcache;
use crate::procps::readproc::{Fill, Proc, Reader};
use crate::procps::{hertz, namespace, scanf, signals, sysinfo};
use crate::stdfd::{self, Stream};

/// `EXIT_FAILURE`: nothing matched, or a pidfile that is not one.
const EXIT_FAILURE: u8 = 1;
/// `EXIT_USAGE`: a command line upstream will not run.
const EXIT_USAGE: u8 = 2;
/// `EXIT_FATAL`: a failure upstream cannot carry on from.
const EXIT_FATAL: u8 = 3;

/// `SIGTERM`, what `pkill` sends when told nothing else.
const SIGTERM: i32 = 15;
/// `_SC_ARG_MAX`, for `sysconf`.
const SC_ARG_MAX: i32 = 0;
/// `get_arg_max`'s bounds: POSIX's minimum, and what xargs clamps to.
const MIN_ARG_SIZE: u64 = 4096;
const MAX_ARG_SIZE: u64 = 128 * 1024;
/// `dev_to_tty`'s buffer in `setDECL(TTY_NAME)`: `char buf[64]`.
const TTY_NAME_BUF: usize = 64;
/// `INT_MAX / 5 / sizeof (struct el)`: where `grow_size` gives up.
const GROW_LIMIT: usize = 2_147_483_647 / 5 / 16;
/// The epoll events `pidwait` asks for at once: `events[32]`.
const EPOLL_BATCH: usize = 32;
/// `pidwait`'s default namespaces to compare: the first six in
/// [`namespace::NS_NAMES`] (`cgroup` to `time`).
const NS_FLAGS_DEFAULT: u32 = 0x3f;

/// What the program is, decided by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Pgrep,
    Pkill,
    Pidwait,
}

impl Mode {
    /// `parse_opts`' test of `program_invocation_short_name`, including the
    /// `lt-` names libtool gives an uninstalled build.
    #[must_use]
    pub fn of(short_name: &[u8]) -> Self {
        match short_name {
            b"pidwait" | b"lt-pidwait" => Self::Pidwait,
            b"pkill" | b"lt-pkill" => Self::Pkill,
            _ => Self::Pgrep,
        }
    }

    /// The mode's own short options, ahead of the common ones.
    fn short_options(self) -> &'static str {
        match self {
            Self::Pgrep => "lad:vwLF:cfinoxP:O:AHg:s:u:U:G:t:r:?Vh",
            Self::Pkill => "eq:LF:cfinoxP:O:AHg:s:u:U:G:t:r:?Vh",
            Self::Pidwait => "eLF:cfinoxP:O:AHg:s:u:U:G:t:r:?Vh",
        }
    }
}

/// `longopts[]`, in upstream's order -- which `getopt_long` lists an
/// ambiguous abbreviation's candidates in.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("signal", Takes::Required),
    ("ignore-ancestors", Takes::Nothing),
    ("require-handler", Takes::Nothing),
    ("count", Takes::Nothing),
    ("cgroup", Takes::Required),
    ("delimiter", Takes::Required),
    ("list-name", Takes::Nothing),
    ("list-full", Takes::Nothing),
    ("full", Takes::Nothing),
    ("pgroup", Takes::Required),
    ("group", Takes::Required),
    ("ignore-case", Takes::Nothing),
    ("newest", Takes::Nothing),
    ("oldest", Takes::Nothing),
    ("older", Takes::Required),
    ("parent", Takes::Required),
    ("session", Takes::Required),
    ("terminal", Takes::Required),
    ("euid", Takes::Required),
    ("uid", Takes::Required),
    ("inverse", Takes::Nothing),
    ("lightweight", Takes::Nothing),
    ("exact", Takes::Nothing),
    ("pidfile", Takes::Required),
    ("logpidfile", Takes::Nothing),
    ("echo", Takes::Nothing),
    ("ns", Takes::Required),
    ("nslist", Takes::Required),
    ("queue", Takes::Required),
    ("runstates", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The status of a usage error, which the getopt crate carries; upstream's
/// own `usage` decides the real one.
const PROGRAM: Program = Program::new("pgrep", 2);

/// What the command line asked for: upstream's file-scope `opt_*` variables.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Opts {
    pub full: bool,
    pub long: bool,
    pub longlong: bool,
    pub oldest: bool,
    /// `-O`: seconds, by `atoi`.
    pub older: i32,
    pub newest: bool,
    pub negate: bool,
    pub exact: bool,
    pub count: bool,
    pub signal: i32,
    pub lock: bool,
    pub case: bool,
    pub echo: bool,
    pub threads: bool,
    pub ns_pid: i32,
    pub use_sigqueue: bool,
    pub require_handler: bool,
    /// `-q`'s `sival_int`.
    pub sigval: i32,
    pub delim: Vec<u8>,
    pub pgrp: Option<Vec<i64>>,
    pub rgid: Option<Vec<i64>>,
    /// The PID read from `-F`'s file.
    pub pid: Option<Vec<i64>>,
    pub ppid: Option<Vec<i64>>,
    pub ignore_ancestors: Option<Vec<i64>>,
    pub sid: Option<Vec<i64>>,
    pub term: Option<Vec<Vec<u8>>>,
    pub euid: Option<Vec<i64>>,
    pub ruid: Option<Vec<i64>>,
    pub nslist: Option<Vec<Vec<u8>>>,
    pub cgroup: Option<Vec<Vec<u8>>>,
    pub pattern: Option<Vec<u8>>,
    pub pidfile: Option<Vec<u8>>,
    pub runstates: Option<Vec<u8>>,
    /// Which namespaces `--ns` compares: bit `i` is [`namespace::NS_NAMES`]`[i]`.
    pub ns_flags: u32,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            full: false,
            long: false,
            longlong: false,
            oldest: false,
            older: 0,
            newest: false,
            negate: false,
            exact: false,
            count: false,
            signal: SIGTERM,
            lock: false,
            case: false,
            echo: false,
            threads: false,
            ns_pid: 0,
            use_sigqueue: false,
            require_handler: false,
            sigval: 0,
            delim: b"\n".to_vec(),
            pgrp: None,
            rgid: None,
            pid: None,
            ppid: None,
            ignore_ancestors: None,
            sid: None,
            term: None,
            euid: None,
            ruid: None,
            nslist: None,
            cgroup: None,
            pattern: None,
            pidfile: None,
            runstates: None,
            ns_flags: NS_FLAGS_DEFAULT,
        }
    }
}

/// One selected process: `struct el`, a PID and (for `-l`, `-a` and `-e`)
/// the name or command line to print with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct El {
    pub num: i64,
    pub str: Vec<u8>,
}

/// Why option parsing stopped short of running.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// `usage (opt)`: the help, on standard error for `'?'` (status 2) and on
    /// standard output otherwise (status 0).
    Usage { to_stderr: bool },
    /// `-V`.
    Version,
    /// A diagnostic has been printed; end with this status.
    Exit(u8),
}

/// The program's state: where `/proc` is, how it was named, and the options.
pub struct Pgrep {
    /// `/proc`, or a test's stand-in for it.
    root: PathBuf,
    /// `argv[0]` exactly: what glibc's `getopt` prefixes its complaints with.
    argv0: Vec<u8>,
    /// `program_invocation_short_name`: what everything else is prefixed with.
    pub name: Vec<u8>,
    pub mode: Mode,
    /// Whether the locale's text is UTF-8, which decides what a regular
    /// expression's `.` is.
    utf8: bool,
    /// The C library's `SIGRTMIN`.
    rtmin: i32,
    /// The password and group files, read when a name is first looked up.
    pw: Option<Pwcache>,
    pub opts: Opts,
}

/// `strict_atol`: a plain decimal number with an optional sign, refused if
/// anything else is there or if it comes near `LONG_MAX`. Nothing at all is
/// 0, as upstream's loop finds.
fn strict_atol(s: &[u8]) -> Option<i64> {
    let (sign, digits) = match s {
        [b'+', rest @ ..] => (1i64, rest),
        [b'-', rest @ ..] => (-1i64, rest),
        _ => (1i64, s),
    };
    let mut res: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        if res >= i64::MAX / 10 {
            return None;
        }
        res = res.checked_mul(10)?;
        let d = i64::from(c.wrapping_sub(b'0'));
        if res >= i64::MAX.checked_sub(d)? {
            return None;
        }
        res = res.checked_add(d)?;
    }
    sign.checked_mul(res)
}

/// `is_long_match`: a pattern of more than fifteen bytes with no `|` or
/// `[` in it, which cannot match a process name the kernel truncates to
/// fifteen.
fn is_long_match(s: Option<&[u8]>) -> bool {
    s.is_some_and(|s| s.len() > 15 && !s.iter().any(|&b| b == b'|' || b == b'['))
}

/// `cgroup_cmp` over `match_cgroup_list`: whether any of a process's cgroup
/// lines, up to the first empty one, is a version-2 line (`0::`) naming one
/// of `list`'s paths.
fn match_cgroup_list(values: Option<&[Vec<u8>]>, list: &[Vec<u8>]) -> bool {
    let Some(values) = values else {
        return false;
    };
    list.iter().rev().any(|path| {
        values
            .iter()
            .take_while(|v| !v.is_empty())
            .any(|v| v.strip_prefix(b"0::") == Some(path.as_slice()))
    })
}

/// `strchr (runstates, state) != NULL`, which a NUL state always satisfies:
/// it finds the string's terminator.
fn state_listed(runstates: &[u8], state: u8) -> bool {
    state == 0 || runstates.contains(&state)
}

/// `strncpy (dst, src, cmdlen - 1)` and the NUL after it: at most
/// `cmdlen - 1` bytes of the C string `src`.
fn cut(src: &[u8], cmdlen: usize) -> &[u8] {
    let s = crate::procps::readproc::cstr(src);
    s.get(..s.len().min(cmdlen.saturating_sub(1)))
        .unwrap_or_default()
}

/// `get_arg_max`: `sysconf (_SC_ARG_MAX)`, read as a `size_t` (so `-1` is
/// huge), held between 4 KiB and 128 KiB.
fn get_arg_max() -> usize {
    let raw = libcall::conf::sysconf(SC_ARG_MAX);
    let val = u64::from_ne_bytes(raw.to_ne_bytes()).clamp(MIN_ARG_SIZE, MAX_ARG_SIZE);
    usize::try_from(val).unwrap_or(usize::MAX)
}

/// `output_numlist`: the PIDs, `delim` between them and a newline after the
/// last.
#[must_use]
pub fn numlist(list: &[El], delim: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, e) in list.iter().enumerate() {
        out.extend_from_slice(e.num.to_string().as_bytes());
        let last = i.saturating_add(1) == list.len();
        out.extend_from_slice(if last { b"\n" } else { delim });
    }
    out
}

/// `output_strlist`: each PID (by `%lu`) and its name or command line.
#[must_use]
pub fn strlist(list: &[El], delim: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, e) in list.iter().enumerate() {
        out.extend_from_slice(
            u64::from_ne_bytes(e.num.to_ne_bytes())
                .to_string()
                .as_bytes(),
        );
        out.push(b' ');
        out.extend_from_slice(&e.str);
        let last = i.saturating_add(1) == list.len();
        out.extend_from_slice(if last { b"\n" } else { delim });
    }
    out
}

/// The text after `program_invocation_short_name` in `usage`, for `mode`.
fn usage_body(mode: Mode) -> &'static str {
    match mode {
        Mode::Pgrep => {
            " -d, --delimiter <string>  specify output delimiter\n \
             -l, --list-name           list PID and process name\n \
             -a, --list-full           list PID and full command line\n \
             -v, --inverse             negates the matching\n \
             -w, --lightweight         list all TID\n"
        }
        Mode::Pkill => {
            " -<sig>                    signal to send (either number or name)\n \
             -H, --require-handler     match only if signal handler is present\n \
             -q, --queue <value>       integer value to be sent with the signal\n \
             -e, --echo                display what is killed\n"
        }
        Mode::Pidwait => " -e, --echo                display PIDs before waiting\n",
    }
}

/// The options every mode lists, and the tail.
const USAGE_COMMON: &str = " -c, --count               count of matching processes\n \
-f, --full                use full process name to match\n \
-g, --pgroup <PGID,...>   match listed process group IDs\n \
-G, --group <GID,...>     match real group IDs\n \
-i, --ignore-case         match case insensitively\n \
-n, --newest              select most recently started\n \
-o, --oldest              select least recently started\n \
-O, --older <seconds>     select where older than seconds\n \
-P, --parent <PPID,...>   match only child processes of the given parent\n \
-s, --session <SID,...>   match session IDs\n     \
--signal <sig>        signal to send (either number or name)\n \
-t, --terminal <tty,...>  match by controlling terminal\n \
-u, --euid <ID,...>       match by effective IDs\n \
-U, --uid <ID,...>        match by real IDs\n \
-x, --exact               match exactly with the command name\n \
-F, --pidfile <file>      read PIDs from file\n \
-L, --logpidfile          fail if PID file is not locked\n \
-r, --runstates <state>   match runstates [D,S,Z,...]\n \
-A, --ignore-ancestors    exclude our ancestors from results\n \
--cgroup <grp,...>        match by cgroup v2 names\n \
--ns <PID>                match the processes that belong to the same\n                           \
namespace as <pid>\n \
--nslist <ns,...>         list which namespaces will be considered for\n                           \
the --ns option.\n                           \
Available namespaces: ipc, mnt, net, pid, user, uts\n\
\n \
-h, --help     display this help and exit\n \
-V, --version  output version information and exit\n\
\nFor more details see pgrep(1).\n";

impl Pgrep {
    /// The program as started with `argv0`, reading processes from `root`.
    ///
    /// `utf8` is whether the locale's text is UTF-8; `rtmin` is the C
    /// library's `SIGRTMIN`.
    #[must_use]
    pub fn new(root: PathBuf, argv0: &[u8], utf8: bool, rtmin: i32) -> Self {
        let name = match argv0.iter().rposition(|&b| b == b'/') {
            Some(at) => argv0.get(at.saturating_add(1)..).unwrap_or_default(),
            None => argv0,
        }
        .to_vec();
        Self {
            root,
            argv0: argv0.to_vec(),
            mode: Mode::of(&name),
            name,
            utf8,
            rtmin,
            pw: None,
            opts: Opts::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Diagnostics: glibc's `error ()`, which flushes standard output first.
    // -----------------------------------------------------------------------

    /// `xwarnx`: `name: message`.
    fn warnx(&self, msg: &[u8]) {
        let mut line = self.name.clone();
        line.extend_from_slice(b": ");
        line.extend_from_slice(msg);
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }

    /// `xwarn`: `name: message: reason`, the reason `errno`'s.
    fn warn(&self, msg: &[u8], errno: i32) {
        let reason = crate::errmsg::strerror(&std::io::Error::from_raw_os_error(errno));
        let mut text = msg.to_vec();
        text.extend_from_slice(b": ");
        text.extend_from_slice(reason.as_bytes());
        self.warnx(&text);
    }

    /// `xerrx (status, …)`: the message, and the status to end with.
    fn errx(&self, status: u8, msg: &[u8]) -> u8 {
        self.warnx(msg);
        status
    }

    /// The help `usage` prints, for this mode and name.
    #[must_use]
    pub fn usage_text(&self) -> Vec<u8> {
        let mut t = b"\nUsage:\n ".to_vec();
        t.extend_from_slice(&self.name);
        t.extend_from_slice(b" [options] <pattern>\n\nOptions:\n");
        t.extend_from_slice(usage_body(self.mode).as_bytes());
        t.extend_from_slice(USAGE_COMMON.as_bytes());
        t
    }

    /// `PROCPS_NG_VERSION`, naming this build.
    #[must_use]
    pub fn version_text(&self) -> Vec<u8> {
        let mut t = self.name.clone();
        t.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
        t
    }

    // -----------------------------------------------------------------------
    // The list options' conversions.
    // -----------------------------------------------------------------------

    /// The password and group database.
    fn pw(&mut self) -> &Pwcache {
        self.pw.get_or_insert_with(Pwcache::load)
    }

    /// `grow_size`: the next capacity of a list, or `integer overflow` (status
    /// 1) once it would pass what upstream can allocate.
    fn grow_size(&self, size: usize) -> Result<usize, u8> {
        if size >= GROW_LIMIT {
            return Err(self.errx(EXIT_FAILURE, b"integer overflow"));
        }
        Ok(size
            .saturating_mul(5)
            .checked_div(4)
            .unwrap_or(0)
            .saturating_add(4))
    }

    /// `split_list`: `s` cut at each comma and each piece converted; `None`
    /// for an empty `s`. A piece that does not convert has already said why,
    /// and ends the run with status 2 -- no usage text.
    fn split_list<T>(
        &mut self,
        s: &[u8],
        mut conv: impl FnMut(&mut Self, &[u8]) -> Option<T>,
    ) -> Result<Option<Vec<T>>, Stop> {
        if s.is_empty() {
            return Ok(None);
        }
        let mut out = Vec::new();
        let mut size = 0usize;
        for piece in s.split(|&b| b == b',') {
            if out.len() == size {
                size = self.grow_size(size).map_err(Stop::Exit)?;
            }
            match conv(self, piece) {
                Some(v) => out.push(v),
                None => return Err(Stop::Exit(EXIT_USAGE)),
            }
        }
        Ok(Some(out))
    }

    /// The prefix-and-name message the conversions warn with.
    fn warn_named(&self, what: &[u8], name: &[u8]) {
        let mut msg = what.to_vec();
        msg.extend_from_slice(name);
        self.warnx(&msg);
    }

    /// `conv_uid`: a number, or a user name.
    fn conv_uid(&mut self, name: &[u8]) -> Option<i64> {
        if let Some(n) = strict_atol(name) {
            return Some(n);
        }
        let uid = self.pw().uid_by_name(name);
        if uid.is_none() {
            self.warn_named(b"invalid user name: ", name);
        }
        uid.map(i64::from)
    }

    /// `conv_gid`: a number, or a group name.
    fn conv_gid(&mut self, name: &[u8]) -> Option<i64> {
        if let Some(n) = strict_atol(name) {
            return Some(n);
        }
        let gid = self.pw().gid_by_name(name);
        if gid.is_none() {
            self.warn_named(b"invalid group name: ", name);
        }
        gid.map(i64::from)
    }

    /// `conv_pgrp`: a number, where 0 is this process's own group.
    fn conv_pgrp(&mut self, name: &[u8]) -> Option<i64> {
        let Some(n) = strict_atol(name) else {
            self.warn_named(b"invalid process group: ", name);
            return None;
        };
        if n != 0 {
            return Some(n);
        }
        match libcall::process::process_group() {
            Ok(pg) => Some(i64::from(pg)),
            // `getpgrp` cannot fail where there are process groups; this is a
            // host without any.
            Err(e) => {
                self.warn(b"getpgrp", e);
                None
            }
        }
    }

    /// `conv_sid`: a number, where 0 is this process's own session.
    fn conv_sid(&mut self, name: &[u8]) -> Option<i64> {
        let Some(n) = strict_atol(name) else {
            self.warn_named(b"invalid session id: ", name);
            return None;
        };
        if n != 0 {
            return Some(n);
        }
        // `getsid (0)` is stored as returned, its failure's -1 included.
        Some(match libcall::process::session_of(0) {
            Ok(sid) => i64::from(sid),
            Err(_) => -1,
        })
    }

    /// `conv_num`: a number.
    fn conv_num(&mut self, name: &[u8]) -> Option<i64> {
        let n = strict_atol(name);
        if n.is_none() {
            self.warn_named(b"not a number: ", name);
        }
        n
    }

    /// `conv_ns`: a namespace name, which *replaces* the set `--ns` compares
    /// -- so of a list only the last name counts, as upstream has it. An
    /// unknown name fails without a word.
    fn conv_ns(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        self.opts.ns_flags = 0;
        let id = namespace::ns_get_id(name)?;
        self.opts.ns_flags |= 1u32.checked_shl(u32::try_from(id).ok()?)?;
        Some(name.to_vec())
    }

    // -----------------------------------------------------------------------
    // parse_opts
    // -----------------------------------------------------------------------

    /// `signal_option`: `pkill`'s first argument, anywhere, that is `-` and a
    /// signal -- which is taken out of the arguments before `getopt` sees them.
    fn take_signal_option(&mut self, args: &mut Vec<OsString>) {
        let found = args.iter().enumerate().find_map(|(i, a)| {
            let bytes = crate::quote::os_bytes(a);
            match bytes.split_first() {
                Some((b'-', rest)) => {
                    let sig = signals::signal_name_to_number(rest, self.rtmin);
                    (sig > -1).then_some((i, sig))
                }
                _ => None,
            }
        });
        if let Some((i, sig)) = found {
            args.remove(i);
            self.opts.signal = sig;
        }
    }

    /// `parse_opts`.
    ///
    /// # Errors
    ///
    /// A [`Stop`] when the run ends here: the help or version was asked for,
    /// the command line is wrong (with the help on standard error), or a
    /// diagnostic has been printed and the status is upstream's.
    #[allow(clippy::too_many_lines)] // upstream's one `switch`, kept whole
    pub fn parse_opts(&mut self, args: &[OsString]) -> Result<(), Stop> {
        let mut args = args.to_vec();
        if self.mode == Mode::Pkill {
            self.take_signal_option(&mut args);
        }
        let mut criteria_count = 0u32;
        let mut operands: Vec<Vec<u8>> = Vec::new();
        let shorts = self.mode.short_options();
        for item in PROGRAM.parse(&args, shorts, LONG_OPTIONS) {
            let item = match item {
                Ok(item) => item,
                Err(e) => {
                    // glibc's `getopt` names the program by `argv[0]` itself.
                    let mut line = self.argv0.clone();
                    line.extend_from_slice(b": ");
                    line.extend_from_slice(e.sentence.as_bytes());
                    line.push(b'\n');
                    stdfd::diag_bytes(&line);
                    return Err(Stop::Usage { to_stderr: true });
                }
            };
            let (opt, arg) = match item {
                Opt::Operand(x) => {
                    operands.push(crate::quote::os_bytes(x).into_owned());
                    continue;
                }
                Opt::Short(c, v) => (Key::Short(c), v),
                Opt::Long(name, v) => (Key::of_long(name), v),
            };
            let arg = arg
                .map(|v| crate::quote::os_bytes(&v).into_owned())
                .unwrap_or_default();
            match opt {
                Key::Signal => self.signal_by_name(&arg)?,
                Key::Short(b'e') => self.opts.echo = true,
                Key::Short(b'F') => {
                    self.opts.pidfile = Some(arg);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'G') => {
                    self.opts.rgid =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_gid)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'L') => self.opts.lock = true,
                Key::Short(b'P') => {
                    self.opts.ppid =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_num)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'U') => {
                    self.opts.ruid =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_uid)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'V') => return Err(Stop::Version),
                Key::Short(b'c') => self.opts.count = true,
                Key::Short(b'd') => self.opts.delim = arg,
                Key::Short(b'f') => self.opts.full = true,
                Key::Short(b'g') => {
                    self.opts.pgrp =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_pgrp)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'i') => {
                    if self.opts.case {
                        // `usage (opt)` with `opt` = 'i': the help, on
                        // standard output, and status 0.
                        return Err(Stop::Usage { to_stderr: false });
                    }
                    self.opts.case = true;
                }
                Key::Short(b'l') => self.opts.long = true,
                Key::Short(b'a') => self.opts.longlong = true,
                Key::Short(b'A') => self.opts.ignore_ancestors = self.ancestors()?,
                Key::Short(c @ (b'n' | b'o')) => {
                    if self.opts.oldest || self.opts.negate || self.opts.newest {
                        return Err(Stop::Usage { to_stderr: true });
                    }
                    if c == b'n' {
                        self.opts.newest = true;
                    } else {
                        self.opts.oldest = true;
                    }
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'O') => {
                    self.opts.older = scanf::atoi(&arg);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b's') => {
                    self.opts.sid =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_sid)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b't') => {
                    self.opts.term = Some(Self::need(Self::split_list(self, &arg, |_, s| {
                        Some(s.to_vec())
                    })?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'u') => {
                    self.opts.euid =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_uid)?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'v') => {
                    if self.opts.oldest || self.opts.negate || self.opts.newest {
                        return Err(Stop::Usage { to_stderr: true });
                    }
                    self.opts.negate = true;
                }
                Key::Short(b'w') => self.opts.threads = true,
                Key::Short(b'x') => self.opts.exact = true,
                Key::Ns => {
                    // `case NS_OPTION:` falls into `case 'r':` when the PID
                    // reads as 0: the argument becomes the run states.
                    self.opts.ns_pid = scanf::atoi(&arg);
                    if self.opts.ns_pid == 0 {
                        self.opts.runstates = Some(arg);
                    }
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'r') => {
                    self.opts.runstates = Some(arg);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::NsList => {
                    self.opts.nslist =
                        Some(Self::need(Self::split_list(self, &arg, Self::conv_ns)?)?);
                }
                Key::Short(b'q') => {
                    self.opts.sigval = scanf::atoi(&arg);
                    self.opts.use_sigqueue = true;
                }
                Key::Cgroup => {
                    self.opts.cgroup = Some(Self::need(Self::split_list(self, &arg, |_, s| {
                        Some(s.to_vec())
                    })?)?);
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'H') => {
                    self.opts.require_handler = true;
                    criteria_count = criteria_count.saturating_add(1);
                }
                Key::Short(b'h') => return Err(Stop::Usage { to_stderr: false }),
                // '?', from `-?`: in the option string, so no complaint first.
                Key::Short(_) => return Err(Stop::Usage { to_stderr: true }),
            }
        }
        self.finish_opts(criteria_count, operands)
    }

    /// `parse_opts` after its loop: the pidfile, and the pattern.
    fn finish_opts(&mut self, criteria_count: u32, operands: Vec<Vec<u8>>) -> Result<(), Stop> {
        let try_help = |s: &Self| {
            let mut t = b"\nTry `".to_vec();
            t.extend_from_slice(&s.name);
            t.extend_from_slice(b" --help' for more information.");
            t
        };
        if self.opts.lock && self.opts.pidfile.is_none() {
            let mut msg = b"-L without -F makes no sense".to_vec();
            msg.extend_from_slice(&try_help(self));
            return Err(Stop::Exit(self.errx(EXIT_USAGE, &msg)));
        }
        if let Some(file) = self.opts.pidfile.clone() {
            match self.read_pidfile(&file) {
                Some(pid) => self.opts.pid = Some(vec![pid]),
                None => {
                    let mut msg = b"pidfile not valid".to_vec();
                    msg.extend_from_slice(&try_help(self));
                    return Err(Stop::Exit(self.errx(EXIT_FAILURE, &msg)));
                }
            }
        }
        let mut operands = operands.into_iter();
        match (operands.next(), operands.next()) {
            (Some(pattern), None) => self.opts.pattern = Some(pattern),
            (Some(_), Some(_)) => {
                let mut msg = b"only one pattern can be provided".to_vec();
                msg.extend_from_slice(&try_help(self));
                return Err(Stop::Exit(self.errx(EXIT_USAGE, &msg)));
            }
            (None, _) if criteria_count == 0 => {
                let mut msg = b"no matching criteria specified".to_vec();
                msg.extend_from_slice(&try_help(self));
                return Err(Stop::Exit(self.errx(EXIT_USAGE, &msg)));
            }
            (None, _) => {}
        }
        Ok(())
    }

    /// A list option's value, which `split_list` returns NULL for only when
    /// it was empty: `usage ('?')`.
    fn need<T>(list: Option<T>) -> Result<T, Stop> {
        list.ok_or(Stop::Usage { to_stderr: true })
    }

    /// `--signal`: a name or number by `signal_name_to_number`, or failing
    /// that anything starting with a digit, by `atoi`.
    fn signal_by_name(&mut self, arg: &[u8]) -> Result<(), Stop> {
        let sig = signals::signal_name_to_number(arg, self.rtmin);
        if sig != -1 {
            self.opts.signal = sig;
            return Ok(());
        }
        if arg.first().is_some_and(u8::is_ascii_digit) {
            self.opts.signal = scanf::atoi(arg);
            return Ok(());
        }
        // `fprintf (stderr, "Unknown signal \"%s\".", optarg)`: no program
        // name and no newline -- the usage that follows starts with one.
        let mut msg = b"Unknown signal \"".to_vec();
        msg.extend_from_slice(arg);
        msg.extend_from_slice(b"\".");
        stdfd::diag_bytes(&msg);
        Err(Stop::Usage { to_stderr: true })
    }

    // -----------------------------------------------------------------------
    // The pidfile
    // -----------------------------------------------------------------------

    /// `read_pidfile`: the PID in `-F`'s file, or `None` when it is not a
    /// regular, non-empty file holding a positive number (followed by nothing
    /// or by a space) in its first eleven bytes -- or, with `-L`, when no
    /// other process holds a lock on it.
    fn read_pidfile(&self, path: &[u8]) -> Option<i64> {
        let mut file = open_pidfile(path)?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() < 1 {
            return None;
        }
        if self.opts.lock && !has_flock(&file) && !has_fcntl(&file) {
            return None;
        }
        let mut buf = [0u8; 11];
        // One `read`, as upstream makes.
        let n = file.read(&mut buf).ok()?;
        if n < 1 {
            return None;
        }
        let text = crate::procps::readproc::cstr(buf.get(..n).unwrap_or_default());
        let (value, used) = scanf::strtoul(text);
        // `int pid = strtoul (…)`: the low 32 bits.
        let pid = scanf::low_i32(i64::from_ne_bytes(value.to_ne_bytes()));
        if used == 0 || pid < 1 {
            return None;
        }
        match text.get(used) {
            Some(&c) if !scanf::is_space(c) => None,
            _ => Some(i64::from(pid)),
        }
    }

    // -----------------------------------------------------------------------
    // The process table
    // -----------------------------------------------------------------------

    /// Every process (or with `threads`, every thread) `procps_pids_get`
    /// would return, read for `fill`. A `/proc` that cannot be listed is no
    /// processes: upstream's `openproc` failing ends its loop at once.
    fn read_table(&self, threads: bool, fill: Fill) -> Vec<Proc> {
        let pw = Pwcache::with_db(pwdb::Db::default());
        let mut reader = Reader::new(self.root.clone(), fill, self.utf8, pw);
        let table = if threads {
            reader.reap_threads(None)
        } else {
            reader.reap()
        };
        // Upstream's own answer, not a guess: `procps_pids_get` returns NULL
        // when `openproc` fails, which ends the loop with nothing matched.
        table.unwrap_or_default()
    }

    /// `get_our_ancestors`: this process's parent, its parent, and so on,
    /// for as long as each is found -- read afresh from `/proc` at each step,
    /// as upstream reads it.
    fn ancestors(&self) -> Result<Option<Vec<i64>>, Stop> {
        // PID and parent are all the walk reads; `stat` and `status` are what
        // decide both, and which processes are seen at all.
        let fill = Fill {
            stat: true,
            status: true,
            ..Fill::default()
        };
        let mut list: Vec<i64> = Vec::new();
        let mut size = 0usize;
        let mut search_pid = getpid();
        loop {
            if list.len() == size {
                size = self.grow_size(size).map_err(Stop::Exit)?;
            }
            let table = self.read_table(false, fill);
            let Some(p) = table.iter().find(|p| p.tid == search_pid) else {
                break;
            };
            if list.contains(&i64::from(p.ppid)) {
                // A cycle, which no kernel writes: see "Deliberate
                // differences" in the module docs.
                break;
            }
            list.push(i64::from(p.ppid));
            search_pid = p.ppid;
        }
        Ok(if list.is_empty() { None } else { Some(list) })
    }

    /// `do_regcomp`: the pattern, anchored by `-x`, compiled `REG_EXTENDED`
    /// (and `REG_ICASE` for `-i`).
    fn regcomp(&self) -> Result<Option<Regex>, u8> {
        let Some(pattern) = &self.opts.pattern else {
            return Ok(None);
        };
        let source = if self.opts.exact {
            [b"^(".as_slice(), pattern, b")$"].concat()
        } else {
            pattern.clone()
        };
        let compiled = if self.utf8 {
            Regex::new_syntax(&source, self.opts.case, Syntax::POSIX_EXTENDED)
        } else {
            Regex::new_syntax_bytes(&source, self.opts.case, Syntax::POSIX_EXTENDED)
        };
        match compiled {
            Ok(re) => Ok(Some(re)),
            Err(e) => {
                let msg = format!("regex error: {}", e.message());
                Err(self.errx(EXIT_USAGE, msg.as_bytes()))
            }
        }
    }

    /// `unhex`: a signal mask's hexadecimal, or 0 with a warning when it is
    /// not entirely that.
    fn unhex(&self, text: &[u8]) -> u64 {
        let (value, used, overflow) = scanf::strtoull_hex(text);
        if overflow || used != text.len() {
            self.warn_named(b"not a hex string: ", text);
            return 0;
        }
        value
    }

    /// `match_signal_handler`: whether the mask catches `signal`. The shift
    /// is C's on x86-64, where the count is taken modulo 64: signal 0 tests
    /// bit 63.
    fn catches(&self, sigcatch: &[u8], signal: i32) -> bool {
        let bit = u32::from_ne_bytes(signal.wrapping_sub(1).to_ne_bytes());
        1u64.wrapping_shl(bit) & self.unhex(sigcatch) != 0
    }

    /// `match_ns`: whether process `pid` shares every namespace `--nslist`
    /// selected with `--ns`'s process.
    fn match_ns(&self, pid: i32, reference: &[u64; 8]) -> Result<bool, u8> {
        let Some(ns) = namespace::ns_read_pid(&self.root, pid) else {
            return Err(self.errx(EXIT_FATAL, b"Unable to read process namespace information"));
        };
        Ok(ns
            .iter()
            .zip(reference)
            .enumerate()
            .all(|(i, (a, b))| self.opts.ns_flags & (1u32 << (i & 31)) == 0 || a == b))
    }

    /// The selection options, in `select_procs`' order: the first that fails
    /// ends the test. `Err` is `match_ns`' fatal error.
    fn passes(&self, p: &Proc, sel: &Selection, devname: &mut Devname) -> Result<bool, u8> {
        let o = &self.opts;
        let num = |list: &Option<Vec<i64>>, v: i64| list.as_ref().is_none_or(|l| l.contains(&v));
        if o.newest && p.start_time < sel.saved_start_time {
            return Ok(false);
        }
        if o.oldest && p.start_time > sel.saved_start_time {
            return Ok(false);
        }
        if !num(&o.ppid, i64::from(p.ppid))
            || !num(&o.pid, i64::from(p.tgid))
            || !num(&o.pgrp, i64::from(p.pgrp))
            || !num(&o.euid, i64::from(p.euid))
            || !num(&o.ruid, i64::from(p.ruid))
            || !num(&o.rgid, i64::from(p.rgid))
            || !num(&o.sid, i64::from(p.session))
        {
            return Ok(false);
        }
        if let Some(reference) = &sel.nsp
            && !self.match_ns(p.tid, reference)?
        {
            return Ok(false);
        }
        if o.older != 0 && cvt_i32(sel.elapsed) < o.older {
            return Ok(false);
        }
        if let Some(terms) = &o.term {
            let dev = u32::from_ne_bytes(p.tty.to_ne_bytes());
            let name = devname.dev_to_tty(TTY_NAME_BUF, dev, p.tid, ABBREV_DEV);
            if !terms.contains(&name) {
                return Ok(false);
            }
        }
        if let Some(states) = &o.runstates
            && !state_listed(states, p.state)
        {
            return Ok(false);
        }
        if let Some(cgroups) = &o.cgroup
            && !match_cgroup_list(p.cgroup_v.as_deref(), cgroups)
        {
            return Ok(false);
        }
        if o.require_handler && !self.catches(&p.sigcatch, o.signal) {
            return Ok(false);
        }
        Ok(true)
    }

    /// `select_procs`: the processes that match, in the order `/proc` lists
    /// them.
    ///
    /// # Errors
    ///
    /// The status to end with, a diagnostic having been printed: a pattern
    /// that does not compile, `--ns`'s process unreadable, or a search the
    /// regex engine abandoned.
    pub fn select_procs(&self) -> Result<Vec<El>, u8> {
        let o = &self.opts;
        let preg = self.regcomp()?;
        let mut sel = Selection {
            saved_start_time: if o.newest { 0 } else { u64::MAX },
            nsp: None,
            elapsed: 0.0,
        };
        let mut saved_pid: i32 = 0;
        if o.oldest {
            saved_pid = i32::MAX;
        }
        if o.ns_pid != 0 {
            match namespace::ns_read_pid(&self.root, o.ns_pid) {
                Some(ns) => sel.nsp = Some(ns),
                None => {
                    return Err(self.errx(
                        EXIT_FATAL,
                        b"Error reading reference namespace information\n",
                    ));
                }
            }
        }
        let fill = Fill {
            stat: true,
            status: true,
            cmdline: true,
            cgroup_v: true,
            ..Fill::default()
        };
        let table = self.read_table(o.threads, fill);
        let myself = getpid();
        let cmdlen = get_arg_max();
        let hz = dbl(u64::try_from(hertz()).unwrap_or(100));
        let mut devname = Devname::new(self.root.clone());
        // Upstream's `cmdoutput` buffer, which keeps its contents from one
        // process to the next.
        let mut cmdoutput: Vec<u8> = Vec::new();
        let mut list: Vec<El> = Vec::new();
        let mut size = 0usize;
        for p in &table {
            // `procps_pids_get` reads the uptime again for each process, and
            // `TIME_ELAPSED` keeps its last value when the new one is not
            // positive -- the subtraction is unsigned, so only when a
            // process started at exactly this tick.
            let boot_tics = cvt_u64(sysinfo::uptime(&self.root) * hz);
            let t = dbl(boot_tics.wrapping_sub(p.start_time));
            if t > 0.0 {
                sel.elapsed = t / hz;
            }
            let pid = p.tid;
            if pid == myself {
                continue;
            }
            if o.ignore_ancestors
                .as_ref()
                .is_some_and(|a| a.contains(&i64::from(pid)))
            {
                continue;
            }
            let mut matched = self.passes(p, &sel, &mut devname)?;
            let cmdline: &[u8] = p.cmdline.as_deref().unwrap_or(b"[ duplicate CMDLINE ]");
            let cmd: &[u8] = p.cmd.as_deref().unwrap_or(b"[ duplicate CMD ]");
            if o.long || o.longlong || (matched && preg.is_some()) {
                cmdoutput = cut(if o.longlong { cmdline } else { cmd }, cmdlen).to_vec();
            }
            if matched && let Some(re) = &preg {
                let search = cut(if o.full { cmdline } else { cmd }, cmdlen);
                match re.is_match(search) {
                    Ok(hit) => matched = hit,
                    Err(limit) => {
                        let msg = format!("regex error: {limit}");
                        return Err(self.errx(EXIT_FATAL, msg.as_bytes()));
                    }
                }
            }
            if matched == o.negate {
                continue;
            }
            if o.newest {
                if sel.saved_start_time == p.start_time && saved_pid > pid {
                    continue;
                }
                sel.saved_start_time = p.start_time;
                saved_pid = pid;
                list.clear();
            }
            if o.oldest {
                if sel.saved_start_time == p.start_time && saved_pid < pid {
                    continue;
                }
                sel.saved_start_time = p.start_time;
                saved_pid = pid;
                list.clear();
            }
            if list.len() == size {
                size = self.grow_size(size)?;
            }
            let str = if o.long || o.longlong || o.echo {
                cmdoutput.clone()
            } else {
                Vec::new()
            };
            list.push(El {
                num: i64::from(pid),
                str,
            });
        }
        if list.is_empty() && !o.full && is_long_match(o.pattern.as_deref()) {
            let mut msg = b"pattern that searches for process name longer than 15 characters \
                            will result in zero matches\nTry `"
                .to_vec();
            msg.extend_from_slice(&self.name);
            msg.extend_from_slice(b" -f' option to match against the complete command line.");
            self.warnx(&msg);
        }
        Ok(list)
    }

    // -----------------------------------------------------------------------
    // main
    // -----------------------------------------------------------------------

    /// `main`: the whole program over `args` (argv without `argv[0]`),
    /// printing to `out`. Returns the exit status, before `close_stdout`.
    pub fn run(&mut self, args: &[OsString], out: &mut Stream) -> u8 {
        // Upstream makes the instance first, under every name (it is built
        // with `ENABLE_PIDWAIT`), and keeps `epoll_create`'s -1 if it fails:
        // `epoll_ctl` on it fails in turn, and nothing is waited for.
        let epollfd = libcall::epoll::create(1).unwrap_or(-1);
        match self.parse_opts(args) {
            Ok(()) => {}
            Err(Stop::Usage { to_stderr }) => {
                let text = self.usage_text();
                return if to_stderr {
                    stdfd::diag_bytes(&text);
                    EXIT_USAGE
                } else {
                    // The status is the write's, and `close_stdout`'s.
                    let _ = out.write_all(&text);
                    0
                };
            }
            Err(Stop::Version) => {
                // As above: `close_stdout` reports a failed write.
                let _ = out.write_all(&self.version_text());
                return 0;
            }
            Err(Stop::Exit(status)) => return status,
        }
        let procs = match self.select_procs() {
            Ok(list) => list,
            Err(status) => return status,
        };
        match self.mode {
            Mode::Pgrep => {
                let text = if self.opts.count {
                    format!("{}\n", procs.len()).into_bytes()
                } else if self.opts.long || self.opts.longlong {
                    strlist(&procs, &self.opts.delim)
                } else {
                    numlist(&procs, &self.opts.delim)
                };
                // Failure to deliver it is `close_stdout`'s to report.
                let _ = out.write_all(&text);
                u8::from(procs.is_empty())
            }
            Mode::Pkill => self.pkill(&procs, out),
            Mode::Pidwait => self.pidwait(&procs, out, epollfd),
        }
    }

    /// `kill` or, with `-q`, `sigqueue`.
    fn execute_kill(&self, pid: i64) -> Result<(), i32> {
        let pid = scanf::low_i32(pid);
        if self.opts.use_sigqueue {
            libcall::sigqueue(pid, self.opts.signal, self.opts.sigval)
        } else {
            libcall::kill(pid, self.opts.signal)
        }
    }

    /// `pkill`'s half of `main`.
    fn pkill(&self, procs: &[El], out: &mut Stream) -> u8 {
        let mut kill_count = 0usize;
        for e in procs {
            match self.execute_kill(e.num) {
                Ok(()) => {
                    if self.opts.echo {
                        let mut line = e.str.clone();
                        line.extend_from_slice(
                            format!(
                                " killed (pid {})\n",
                                u64::from_ne_bytes(e.num.to_ne_bytes())
                            )
                            .as_bytes(),
                        );
                        // `close_stdout` reports a failed write.
                        let _ = out.write_all(&line);
                    }
                    kill_count = kill_count.saturating_add(1);
                }
                // Gone now, which is OK.
                Err(libcall::ESRCH) => {}
                Err(errno) => {
                    self.warn(format!("killing pid {} failed", e.num).as_bytes(), errno);
                }
            }
        }
        if self.opts.count {
            // As above.
            let _ = out.write_all(format!("{}\n", procs.len()).as_bytes());
        }
        u8::from(kill_count == 0)
    }

    /// `pidwait`'s half of `main`: a `pidfd` for each process, all of them
    /// watched by one epoll instance until each has reported its exit.
    fn pidwait(&self, procs: &[El], out: &mut Stream, epollfd: i32) -> u8 {
        if self.opts.count {
            // `close_stdout` reports a failed write.
            let _ = out.write_all(format!("{}\n", procs.len()).as_bytes());
        }
        let mut poll_count: i64 = 0;
        for e in procs {
            if self.opts.echo {
                let mut line = b"waiting for ".to_vec();
                line.extend_from_slice(&e.str);
                line.extend_from_slice(
                    format!(" (pid {})\n", u64::from_ne_bytes(e.num.to_ne_bytes())).as_bytes(),
                );
                // As above.
                let _ = out.write_all(&line);
            }
            let pidfd = match libcall::process::pidfd_open(scanf::low_i32(e.num), 0) {
                Ok(fd) => fd,
                Err(libcall::ENOSYS) => {
                    return self.errx(EXIT_FAILURE, b"pidfd_open() not implemented in Linux < 5.3");
                }
                // Gone already, as for `pkill`.
                Err(libcall::ESRCH) => continue,
                Err(errno) => {
                    self.warn(format!("opening pid {} failed", e.num).as_bytes(), errno);
                    continue;
                }
            };
            let cookie = u64::from(u32::from_ne_bytes(pidfd.to_ne_bytes()));
            let events = libcall::epoll::EPOLLIN | libcall::epoll::EPOLLET;
            if libcall::epoll::add(epollfd, pidfd, events, cookie).is_ok() {
                poll_count = poll_count.saturating_add(1);
            }
        }
        let mut wait_count: i64 = 0;
        let mut events = [libcall::epoll::EpollEvent::default(); EPOLL_BATCH];
        while wait_count < poll_count {
            match libcall::epoll::wait(epollfd, &mut events, -1) {
                Ok(n) => wait_count = wait_count.saturating_add(i64::try_from(n).unwrap_or(0)),
                Err(libcall::EINTR) => {}
                Err(errno) => {
                    // Upstream warns and then adds `epoll_wait`'s -1 to its
                    // count, as here.
                    self.warn(b"epoll_wait failed", errno);
                    wait_count = wait_count.saturating_sub(1);
                }
            }
        }
        u8::from(wait_count == 0)
    }
}

/// The parts of `select_procs`' state that the selection tests read.
struct Selection {
    saved_start_time: u64,
    /// `--ns`'s process's namespaces.
    nsp: Option<[u64; 8]>,
    /// The stack's `TIME_ELAPSED`, which survives from one process to the
    /// next when not set.
    elapsed: f64,
}

/// An option as `parse_opts`' `switch` sees it: the long options without a
/// short one have values of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Short(u8),
    Signal,
    Ns,
    NsList,
    Cgroup,
}

impl Key {
    /// The `val` of `longopts[]` for the option the table spells `name`.
    fn of_long(name: &str) -> Self {
        match name {
            "signal" => Self::Signal,
            "ns" => Self::Ns,
            "nslist" => Self::NsList,
            "cgroup" => Self::Cgroup,
            "ignore-ancestors" => Self::Short(b'A'),
            "require-handler" => Self::Short(b'H'),
            "count" => Self::Short(b'c'),
            "delimiter" => Self::Short(b'd'),
            "list-name" => Self::Short(b'l'),
            "list-full" => Self::Short(b'a'),
            "full" => Self::Short(b'f'),
            "pgroup" => Self::Short(b'g'),
            "group" => Self::Short(b'G'),
            "ignore-case" => Self::Short(b'i'),
            "newest" => Self::Short(b'n'),
            "oldest" => Self::Short(b'o'),
            "older" => Self::Short(b'O'),
            "parent" => Self::Short(b'P'),
            "session" => Self::Short(b's'),
            "terminal" => Self::Short(b't'),
            "euid" => Self::Short(b'u'),
            "uid" => Self::Short(b'U'),
            "inverse" => Self::Short(b'v'),
            "lightweight" => Self::Short(b'w'),
            "exact" => Self::Short(b'x'),
            "pidfile" => Self::Short(b'F'),
            "logpidfile" => Self::Short(b'L'),
            "echo" => Self::Short(b'e'),
            "queue" => Self::Short(b'q'),
            "runstates" => Self::Short(b'r'),
            "version" => Self::Short(b'V'),
            // "help", and nothing else the table holds.
            _ => Self::Short(b'h'),
        }
    }
}

/// This process's ID, as `getpid` returns it.
fn getpid() -> i32 {
    scanf::low_i32(i64::from(std::process::id()))
}

/// `open (path, O_RDONLY | O_NOCTTY | O_NONBLOCK)`: not blocking on a FIFO,
/// and not taking a terminal as this process's.
#[cfg(unix)]
fn open_pidfile(path: &[u8]) -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOCTTY: i32 = 0o400;
    const O_NONBLOCK: i32 = 0o4000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOCTTY | O_NONBLOCK)
        .open(crate::quote::os_from_bytes(path))
        .ok()
}

/// The host has no FIFOs or terminals to avoid.
#[cfg(not(unix))]
fn open_pidfile(path: &[u8]) -> Option<std::fs::File> {
    std::fs::File::open(crate::quote::os_from_bytes(path)).ok()
}

/// `has_flock`: whether another process holds an `flock` that a shared one
/// conflicts with. Taking it when nobody does keeps it until the file closes,
/// as upstream's does.
fn has_flock(file: &std::fs::File) -> bool {
    matches!(
        file.try_lock_shared(),
        Err(std::fs::TryLockError::WouldBlock)
    )
}

/// `has_fcntl`: the same question of a POSIX record lock, which says
/// `EACCES` or `EAGAIN`.
#[cfg(unix)]
fn has_fcntl(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    matches!(
        libcall::lock::set_read_lock(file.as_raw_fd()),
        Err(libcall::EACCES | libcall::EAGAIN)
    )
}

/// The host has no record locks to ask about.
#[cfg(not(unix))]
fn has_fcntl(_file: &std::fs::File) -> bool {
    false
}

/// The program, as `bin/pgrep.rs` and `bin/pkill.rs` run it.
#[must_use]
pub fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| crate::quote::os_bytes(a).into_owned())
        .unwrap_or_default();
    let mut prog = Pgrep::new(
        PathBuf::from("/proc"),
        &argv0,
        crate::locale::ctype_is_utf8(),
        libcall::sigrtmin(),
    );
    let mut out = Stream::stdout();
    let status = prog.run(argv.get(1..).unwrap_or_default(), &mut out);
    stdfd::close_stdout_bytes(&prog.name, out, ExitCode::from(status), EXIT_FAILURE)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn prog(name: &str, root: &Path) -> Pgrep {
        Pgrep::new(root.to_path_buf(), name.as_bytes(), true, 34)
    }

    fn parsed(name: &str, args: &[&str]) -> Result<Opts, Stop> {
        let mut p = prog(name, Path::new("no-such-proc"));
        p.parse_opts(&os(args)).map(|()| p.opts)
    }

    /// One fake process: `stat`, `status`, `cmdline` and `cgroup`.
    struct Fake<'a> {
        pid: u32,
        comm: &'a str,
        ppid: u32,
        uid: u32,
        state: char,
        start: u64,
        cmdline: &'a [u8],
        cgroup: &'a [u8],
    }

    /// A fake `/proc` with a few processes, none of them this one.
    ///
    /// | pid | name | ppid | uid | state | start | cgroup |
    /// |---|---|---|---|---|---|---|
    /// | 70001 | `bash` | 1 | 0 | S | 100 | `0::/user.slice` |
    /// | 70003 | `vim` | 70001 | 1000 | S | 500 | `0::/user.slice` |
    /// | 70005 | `sshd` | 1 | 0 | S | 50 | a v1 line, then `0::/system.slice/ssh` |
    /// | 70007 | `make` | 70001 | 1000 | Z | 900 | an empty line before `0::/x` |
    /// | 70009 | `averyveryverylo` | 1 | 1000 | R | 900 | empty |
    fn world(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lane-b-pgrep-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let put = |path: &str, text: &[u8]| {
            let p = root.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        let procs = [
            Fake {
                pid: 70001,
                comm: "bash",
                ppid: 1,
                uid: 0,
                state: 'S',
                start: 100,
                cmdline: b"-bash\0",
                cgroup: b"0::/user.slice\n",
            },
            Fake {
                pid: 70003,
                comm: "vim",
                ppid: 70001,
                uid: 1000,
                state: 'S',
                start: 500,
                cmdline: b"vim\0notes.txt\0",
                cgroup: b"0::/user.slice\n",
            },
            Fake {
                pid: 70005,
                comm: "sshd",
                ppid: 1,
                uid: 0,
                state: 'S',
                start: 50,
                cmdline: b"/usr/sbin/sshd\0-D\0",
                cgroup: b"12:pids:/system.slice\n0::/system.slice/ssh\n",
            },
            Fake {
                pid: 70007,
                comm: "make",
                ppid: 70001,
                uid: 1000,
                state: 'Z',
                start: 900,
                cmdline: b"",
                cgroup: b"1:x:/\n\n0::/x\n",
            },
            Fake {
                pid: 70009,
                comm: "averyveryverylo",
                ppid: 1,
                uid: 1000,
                state: 'R',
                start: 900,
                cmdline: b"averyveryverylongname\0--go\0",
                cgroup: b"",
            },
        ];
        for f in &procs {
            let Fake {
                pid,
                comm,
                ppid,
                uid,
                state,
                start,
                ..
            } = *f;
            put(
                &format!("{pid}/stat"),
                format!(
                    "{pid} ({comm}) {state} {ppid} {pid} {pid} 0 -1 4194560 0 0 0 0 1 2 0 0 \
                     20 0 1 0 {start} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0\n"
                )
                .as_bytes(),
            );
            put(
                &format!("{pid}/status"),
                format!(
                    "Name:\t{comm}\nState:\t{state} (x)\nTgid:\t{pid}\nPid:\t{pid}\n\
                     PPid:\t{ppid}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n\
                     Gid:\t{uid}\t{uid}\t{uid}\t{uid}\nSigCgt:\t0000000000004000\n"
                )
                .as_bytes(),
            );
            put(&format!("{pid}/cmdline"), f.cmdline);
            put(&format!("{pid}/cgroup"), f.cgroup);
        }
        put("uptime", b"1000.00 2000.00\n");
        root
    }

    fn pids(name: &str, root: &Path, args: &[&str]) -> Vec<i64> {
        let mut p = prog(name, root);
        p.parse_opts(&os(args)).unwrap();
        p.select_procs()
            .unwrap()
            .into_iter()
            .map(|e| e.num)
            .collect()
    }

    #[test]
    fn the_name_decides_the_program() {
        assert_eq!(Mode::of(b"pgrep"), Mode::Pgrep);
        assert_eq!(Mode::of(b"pkill"), Mode::Pkill);
        assert_eq!(Mode::of(b"lt-pkill"), Mode::Pkill);
        assert_eq!(Mode::of(b"pidwait"), Mode::Pidwait);
        assert_eq!(Mode::of(b"pkill.exe"), Mode::Pgrep);
        assert_eq!(Mode::of(b""), Mode::Pgrep);
        let p = Pgrep::new(PathBuf::from("/proc"), b"/usr/bin/pkill", true, 34);
        assert_eq!(p.name, b"pkill");
        assert_eq!(p.mode, Mode::Pkill);
    }

    #[test]
    fn strict_atol_is_upstreams() {
        assert_eq!(strict_atol(b"42"), Some(42));
        assert_eq!(strict_atol(b"-7"), Some(-7));
        assert_eq!(strict_atol(b"+7"), Some(7));
        assert_eq!(strict_atol(b""), Some(0));
        assert_eq!(strict_atol(b"-"), Some(0));
        assert_eq!(strict_atol(b"1x"), None);
        assert_eq!(strict_atol(b" 1"), None);
        assert_eq!(
            strict_atol(b"999999999999999999"),
            Some(999_999_999_999_999_999)
        );
        // Its eighteen-digit prefix is past `LONG_MAX / 10`.
        assert_eq!(strict_atol(b"9999999999999999999"), None);
    }

    #[test]
    fn long_patterns_and_cgroup_lines() {
        assert!(!is_long_match(None));
        assert!(!is_long_match(Some(b"fifteen-bytes-x")));
        assert!(is_long_match(Some(b"sixteen-bytes-xx")));
        assert!(!is_long_match(Some(b"sixteen-bytes|xx")));
        assert!(!is_long_match(Some(b"sixteen-bytes[xx")));
        let v = vec![b"1:x:/".to_vec(), b"0::/a".to_vec(), Vec::new()];
        assert!(match_cgroup_list(Some(&v), &[b"/a".to_vec()]));
        assert!(!match_cgroup_list(Some(&v), &[b"a".to_vec()]));
        let gap = vec![b"1:x:/".to_vec(), Vec::new(), b"0::/a".to_vec()];
        assert!(!match_cgroup_list(Some(&gap), &[b"/a".to_vec()]));
        assert!(!match_cgroup_list(None, &[b"/a".to_vec()]));
        assert!(state_listed(b"SR", b'R'));
        assert!(!state_listed(b"SR", b'Z'));
        assert!(state_listed(b"", 0));
    }

    #[test]
    fn lists_print_as_upstream_prints_them() {
        let list = [
            El {
                num: 5,
                str: b"a".to_vec(),
            },
            El {
                num: -1,
                str: b"b c".to_vec(),
            },
        ];
        assert_eq!(numlist(&list, b","), b"5,-1\n");
        assert_eq!(numlist(&list, b""), b"5-1\n");
        assert_eq!(strlist(&list, b"\n"), b"5 a\n18446744073709551615 b c\n");
        assert_eq!(numlist(&[], b","), b"");
    }

    #[test]
    fn the_help_is_the_modes() {
        let text = String::from_utf8(prog("pkill", Path::new(".")).usage_text()).unwrap();
        assert!(text.starts_with("\nUsage:\n pkill [options] <pattern>\n\nOptions:\n -<sig>"));
        assert!(text.contains("\n     --signal <sig>        signal to send"));
        assert!(text.contains(
            " --ns <PID>                match the processes that belong to the same\n                           namespace as <pid>\n"
        ));
        assert!(text.ends_with(
            "\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see pgrep(1).\n"
        ));
        assert!(!text.contains("--list-name"));
        let g = String::from_utf8(prog("pgrep", Path::new(".")).usage_text()).unwrap();
        assert!(g.contains(" -w, --lightweight         list all TID\n -c, --count"));
    }

    #[test]
    fn pkill_takes_its_signal_from_anywhere() {
        assert_eq!(parsed("pkill", &["-9", "x"]).unwrap().signal, 9);
        assert_eq!(parsed("pkill", &["x", "-HUP"]).unwrap().signal, 1);
        assert_eq!(parsed("pkill", &["-c", "-usr1", "x"]).unwrap().signal, 10);
        // Only the first: a second is an invalid option.
        assert_eq!(
            parsed("pkill", &["-9", "-9", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
        // `pgrep` has no such argument.
        assert_eq!(
            parsed("pgrep", &["-9", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
        assert_eq!(
            parsed("pkill", &["--signal", "KILL", "x"]).unwrap().signal,
            9
        );
        assert_eq!(
            parsed("pkill", &["--signal", "200", "x"]).unwrap().signal,
            200
        );
        assert_eq!(parsed("pkill", &["--signal", "9x", "x"]).unwrap().signal, 9);
        assert_eq!(
            parsed("pkill", &["--signal", "NOPE", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
    }

    #[test]
    fn upstreams_option_quirks_are_kept() {
        // `-i` twice is the help on standard output.
        assert_eq!(
            parsed("pgrep", &["-i", "-i", "x"]),
            Err(Stop::Usage { to_stderr: false })
        );
        assert_eq!(
            parsed("pgrep", &["-n", "-v", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
        assert_eq!(
            parsed("pgrep", &["-v", "-o", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
        // `--ns 0` falls through to `--runstates`.
        let o = parsed("pgrep", &["--ns", "0"]).unwrap();
        assert_eq!((o.ns_pid, o.runstates), (0, Some(b"0".to_vec())));
        let o = parsed("pgrep", &["--ns", "5"]).unwrap();
        assert_eq!((o.ns_pid, o.runstates), (5, None));
        // `-O` is `atoi`, and counts as a criterion whatever it reads.
        assert_eq!(parsed("pgrep", &["-O", "abc"]).unwrap().older, 0);
        // Only the last namespace of a list counts.
        assert_eq!(
            parsed("pgrep", &["--nslist", "net,pid", "x"])
                .unwrap()
                .ns_flags,
            1 << 4
        );
        assert_eq!(
            parsed("pgrep", &["--nslist", "net,nope", "x"]),
            Err(Stop::Exit(2))
        );
        // Long options are everybody's.
        assert!(parsed("pkill", &["--inverse", "x"]).unwrap().negate);
        assert_eq!(
            parsed("pkill", &["-v", "x"]),
            Err(Stop::Usage { to_stderr: true })
        );
        assert_eq!(
            parsed("pgrep", &["-?"]),
            Err(Stop::Usage { to_stderr: true })
        );
        assert_eq!(
            parsed("pgrep", &["-h"]),
            Err(Stop::Usage { to_stderr: false })
        );
        assert_eq!(parsed("pgrep", &["-V", "-z"]), Err(Stop::Version));
        assert_eq!(
            parsed("pgrep", &["-z", "-V"]),
            Err(Stop::Usage { to_stderr: true })
        );
    }

    #[test]
    fn the_command_line_must_say_what_to_match() {
        assert_eq!(parsed("pgrep", &[]), Err(Stop::Exit(2)));
        assert_eq!(parsed("pgrep", &["a", "b"]), Err(Stop::Exit(2)));
        assert_eq!(parsed("pgrep", &["-L", "x"]), Err(Stop::Exit(2)));
        assert_eq!(
            parsed("pgrep", &["-F", "no-such-pidfile"]),
            Err(Stop::Exit(1))
        );
        assert_eq!(
            parsed("pgrep", &["-P", ""]),
            Err(Stop::Usage { to_stderr: true })
        );
        assert_eq!(parsed("pgrep", &["-P", "1,x"]), Err(Stop::Exit(2)));
        assert_eq!(
            parsed("pgrep", &["-P", "1,,2"]).unwrap().ppid,
            Some(vec![1, 0, 2])
        );
        assert_eq!(parsed("pgrep", &["-c"]), Err(Stop::Exit(2)));
        assert!(parsed("pgrep", &["-r", "Z"]).is_ok());
    }

    #[test]
    fn pidfiles_hold_one_positive_number() {
        let dir = std::env::temp_dir().join(format!("lane-b-pgrep-pidfile-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p");
        let check = |content: &[u8]| {
            fs::write(&f, content).unwrap();
            prog("pgrep", Path::new(".")).read_pidfile(f.to_str().unwrap().as_bytes())
        };
        assert_eq!(check(b"123\n"), Some(123));
        assert_eq!(check(b"  77 junk"), Some(77));
        assert_eq!(check(b"12x"), None);
        assert_eq!(check(b"0\n"), None);
        assert_eq!(check(b"-5"), None);
        assert_eq!(check(b"4294967297"), Some(1));
        assert_eq!(check(b""), None);
        assert_eq!(check(b"123456789012"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn selection_follows_upstreams_order() {
        let root = world("select");
        let none: Vec<i64> = Vec::new();
        assert_eq!(pids("pgrep", &root, &["bash"]), [70001]);
        assert_eq!(pids("pgrep", &root, &["-x", "bas"]), none);
        assert_eq!(pids("pgrep", &root, &["-i", "BASH"]), [70001]);
        assert_eq!(pids("pgrep", &root, &["-f", "notes"]), [70003]);
        assert_eq!(pids("pgrep", &root, &["-P", "70001"]), [70003, 70007]);
        assert_eq!(pids("pgrep", &root, &["-u", "1000", "-r", "Z"]), [70007]);
        assert_eq!(pids("pgrep", &root, &["-n", "-u", "1000"]), [70009]);
        assert_eq!(pids("pgrep", &root, &["-o"]), [70005]);
        assert_eq!(
            pids("pgrep", &root, &["-v", "-u", "0"]),
            [70003, 70007, 70009]
        );
        assert_eq!(
            pids("pgrep", &root, &["--cgroup", "/system.slice/ssh"]),
            [70005]
        );
        // An empty line ends the search through a process's cgroups.
        assert_eq!(pids("pgrep", &root, &["--cgroup", "/x"]), none);
        // SigCgt 0x4000 is SIGTERM (bit 14).
        assert_eq!(
            pids("pgrep", &root, &["-H", "-u", "1000"]),
            [70003, 70007, 70009]
        );
        assert_eq!(pids("pgrep", &root, &["-H", "--signal", "HUP"]), none);
        // Started at 900 ticks, read at 100000: 991 seconds old.
        assert_eq!(pids("pgrep", &root, &["-O", "992"]), [70001, 70003, 70005]);
        // The process name is fifteen bytes; the command line is not.
        assert_eq!(pids("pgrep", &root, &["averyveryverylongname"]), none);
        assert_eq!(
            pids("pgrep", &root, &["-f", "averyveryverylongname"]),
            [70009]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_and_command_lines_go_with_the_pids() {
        let root = world("names");
        let mut p = prog("pgrep", &root);
        p.parse_opts(&os(&["-a", "-u", "1000"])).unwrap();
        let got: Vec<(i64, Vec<u8>)> = p
            .select_procs()
            .unwrap()
            .into_iter()
            .map(|e| (e.num, e.str))
            .collect();
        assert_eq!(
            got,
            [
                (70003, b"vim notes.txt".to_vec()),
                (70007, b"[make] <defunct>".to_vec()),
                (70009, b"averyveryverylongname --go".to_vec()),
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// `-A`'s walk from this process up its parents, over a fixture holding
    /// this process, and an ancestor loop no kernel would write.
    #[test]
    fn ancestors_are_walked_and_a_loop_ends_the_walk() {
        let me = std::process::id();
        let root = std::env::temp_dir().join(format!("lane-b-pgrep-anc-{me}"));
        let _ = fs::remove_dir_all(&root);
        let put = |pid: u32, ppid: u32| {
            let d = root.join(pid.to_string());
            fs::create_dir_all(&d).unwrap();
            fs::write(
                d.join("stat"),
                format!("{pid} (p) S {ppid} {pid} {pid} 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1\n"),
            )
            .unwrap();
            fs::write(
                d.join("status"),
                format!("Name:\tp\nPid:\t{pid}\nPPid:\t{ppid}\nUid:\t0\t0\t0\t0\n"),
            )
            .unwrap();
        };
        // A chain that ends at a parent that is not there.
        put(me, 70013);
        put(70013, 70015);
        put(70015, 1);
        let p = prog("pgrep", &root);
        assert_eq!(p.ancestors(), Ok(Some(vec![70013, 70015, 1])));
        // A loop: this process's parent's parent is this process.
        put(70013, me);
        assert_eq!(p.ancestors(), Ok(Some(vec![70013, i64::from(me)])));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bad_pattern_is_a_usage_error() {
        let root = world("badre");
        let mut p = prog("pgrep", &root);
        p.parse_opts(&os(&["("])).unwrap();
        assert_eq!(p.select_procs(), Err(2));
        let _ = fs::remove_dir_all(&root);
    }
}
