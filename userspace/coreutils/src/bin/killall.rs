//! `killall` -- send a signal to processes by name: psmisc 23.7's, ported.
//!
//! ```text
//! killall [OPTION]... [--] NAME...
//! killall -l, --list
//! killall -V, --version
//! ```
//!
//! Ubuntu builds psmisc with no patches and with SELinux, so this is the
//! `killall` Ubuntu ships, file for file: `src/killall.c` here, and
//! `src/signals.c` with the table `signames.h` is generated into at build
//! time, from glibc's `<signal.h>` on x86-64. It came back after the one
//! `kill` of design-decisions §1072 retired `userspace/kill`, whose `killall`
//! ended every process it named on the spot.
//!
//! # How upstream decides
//!
//! The process table is `/proc` as `readdir` lists it, this process left out.
//! For each process upstream reads `stat` for its name -- the first `(` to the
//! last `)`, cut to 63 bytes -- and, when that name is 15 or 63 bytes long
//! and so maybe cut short by the kernel, `cmdline` for the full one: the
//! first argument whose last path component begins with the cut name. A NAME
//! with a `/` is a file instead, and matches a process whose `exe` is that
//! file (by device and inode, or failing that by the link's text). `-r`
//! makes each NAME an extended regular expression, `-I` folds case, `-e`
//! skips a process whose long name could not be confirmed, `-u`, `-n`, `-y`,
//! `-o` and `-Z` narrow the table by owner, PID namespace, age and security
//! context. `-g` signals each matching process's group once. The status is 0
//! when every NAME found a process (with no NAME, when anything was
//! signalled), else 1.
//!
//! The command line is `getopt_long_only`'s with `opterr` 0, and upstream
//! decides what each refusal was by looking at the word itself: `-HUP` and
//! `-9` are signals, `-ve` is `-v -e`, anything else is the usage. `-I` and
//! `-V` are checked again, because `-INT` and `-VTALRM` reach them as
//! bundles: when the word was not `-I`/`-V` or a `--` spelling, the *next*
//! word, less its first byte, is taken for a signal name -- upstream's own
//! comment, "option check is optind-1 but sig name is optind". All of it is
//! kept.
//!
//! # Deliberate differences
//!
//! 1. **`-V` names this build**, `killall (SlateOS coreutils) 0.1.0`, as
//!    `pstree` does.
//! 2. **A process whose group cannot be read is not signalled.** Upstream
//!    goes on with the group -1 and signals `-(-1)`: the process itself
//!    through its pidfd, but where the system has no pidfds -- SlateOS --
//!    process 1. Here it is skipped, after upstream's message.
//! 3. **`-I` or `-V` spelled long with one dash and nothing after it**
//!    (`killall -ignore-case`) reads no word past the end: upstream takes
//!    `argv[optind] + 1` from a null pointer and dies of `SIGSEGV`. Here the
//!    missing word is an unknown signal, the run's usual answer to one.
//! 4. **A failed write to standard output is reported** -- the prompts of
//!    `-i`, the list of `-l` -- `killall: write error: ...`, status 1
//!    (design-decisions §1071). Upstream registers no `close_stdout`.
//! 5. **A regular expression the engine gives up on** -- a backreference
//!    search past its step budget -- ends the run, `killall: regex error`,
//!    status 1, where glibc has no budget and answers eventually.
//!
//! On SlateOS a process is signalled with `kill(2)`: its kernel has no
//! pidfds, which is the case upstream falls back to `kill` for
//! ([`libcall::process::pidfd_send_signal`]).
//!
//! `scripts/killall-diff.sh` holds it to Ubuntu's, each case in a PID
//! namespace of its own, so the processes either side can signal are the
//! case's own.

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use ere::{Regex, Syntax};

coreutils::guard_std_fds!();

/// The parser's name; it never prints (`opterr` is 0).
const KILLALL: Program = Program::new("killall", 1);

/// Upstream's option string.
const SHORTS: &str = "egy:o:ilqrs:u:vwZ:VIn:";

/// Upstream's `options[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("exact", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("process-group", Takes::Nothing),
    ("younger-than", Takes::Required),
    ("older-than", Takes::Required),
    ("interactive", Takes::Nothing),
    ("list-signals", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("regexp", Takes::Nothing),
    ("signal", Takes::Required),
    ("user", Takes::Required),
    ("verbose", Takes::Nothing),
    ("wait", Takes::Nothing),
    ("ns", Takes::Required),
    ("context", Takes::Required),
    ("version", Takes::Nothing),
];

/// `COMM_LEN`: the longest name `stat` gives, with its NUL.
const COMM_LEN: usize = 64;
/// `OLD_COMM_LEN`: the kernel's `TASK_COMM_LEN`, with its NUL.
const OLD_COMM_LEN: usize = 16;
/// `MAX_NAMES`: one bit of an `unsigned long` per NAME.
const MAX_NAMES: usize = 64;
/// `SIGTERM`.
const SIGTERM: i32 = 15;
/// `ESRCH`.
const ESRCH: i32 = 3;
/// `ENOSYS`.
const ENOSYS: i32 = 38;

/// `signames.h`, as psmisc's build generates it on x86-64: every `SIG*`
/// `<signal.h>` defines as a number below 100, in number order -- no
/// aliases (`IOT`, `CLD`, `IO` are defined as other signals) and nothing
/// real-time.
const SIGNALS: [(i32, &str); 31] = [
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (4, "ILL"),
    (5, "TRAP"),
    (6, "ABRT"),
    (7, "BUS"),
    (8, "FPE"),
    (9, "KILL"),
    (10, "USR1"),
    (11, "SEGV"),
    (12, "USR2"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (16, "STKFLT"),
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
    (21, "TTIN"),
    (22, "TTOU"),
    (23, "URG"),
    (24, "XCPU"),
    (25, "XFSZ"),
    (26, "VTALRM"),
    (27, "PROF"),
    (28, "WINCH"),
    (29, "POLL"),
    (30, "PWR"),
    (31, "SYS"),
];

/// How a run ends early: the status, after whatever was said.
struct Exit(u8);

/// What the program asks of the system beyond `/proc`: a fake in the tests,
/// which must signal nothing.
trait System {
    /// Send `sig` to `pid` -- as upstream's `my_send_signal`: through the
    /// pidfd of the process's `/proc` directory when `pid` is positive and
    /// the system has pidfds, else `kill(2)`.
    fn send(&self, dir: Option<&std::fs::File>, pid: i32, sig: i32) -> Result<(), i32>;
    /// `kill (pid, 0) < 0 && errno == ESRCH`: whether `pid` (a group when
    /// negative) is gone.
    fn gone(&self, pid: i32) -> bool;
    /// `getpgid (pid)`.
    fn getpgid(&self, pid: i32) -> Result<i32, i32>;
    /// `getpid ()`.
    fn getpid(&self) -> i32;
    /// `sysconf (_SC_CLK_TCK)`.
    fn clk_tck(&self) -> f64;
    /// `sleep (1)`, in `-w`'s wait.
    fn sleep_a_second(&self);
}

/// The system.
struct Libc;

impl System for Libc {
    fn send(&self, dir: Option<&std::fs::File>, pid: i32, sig: i32) -> Result<(), i32> {
        if pid > 0
            && let Some(dir) = dir
        {
            match libcall::process::pidfd_send_signal(raw_fd(dir), sig) {
                Err(ENOSYS) => {}
                other => return other,
            }
        }
        libcall::kill_any(pid, sig)
    }

    fn gone(&self, pid: i32) -> bool {
        libcall::kill_any(pid, 0) == Err(ESRCH)
    }

    fn getpgid(&self, pid: i32) -> Result<i32, i32> {
        libcall::process::getpgid(pid)
    }

    fn getpid(&self) -> i32 {
        i32::try_from(std::process::id()).unwrap_or(0)
    }

    fn clk_tck(&self) -> f64 {
        // `sysconf` answers a small integer; the cast is upstream's own
        // `double sc_clk_tck = sysconf (_SC_CLK_TCK)`.
        #[allow(clippy::cast_precision_loss)]
        let ticks = libcall::conf::sysconf(libcall::conf::SC_CLK_TCK) as f64;
        ticks
    }

    fn sleep_a_second(&self) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// What `open_dir` found at a `/proc/PID`. Each host makes one of the two,
/// which is why each variant is unused on the other.
enum ProcDir {
    /// It opened: the descriptor `pidfd_send_signal` is given.
    #[cfg_attr(not(unix), allow(dead_code))] // only Unix opens a directory
    Open(std::fs::File),
    /// It is there, on a host that opens no directory as a file.
    #[cfg_attr(unix, allow(dead_code))] // Unix opens it instead
    Present,
}

impl ProcDir {
    /// The descriptor to signal through, if there is one.
    fn file(&self) -> Option<&std::fs::File> {
        match self {
            ProcDir::Open(file) => Some(file),
            ProcDir::Present => None,
        }
    }
}

/// `open (path, O_RDONLY|O_DIRECTORY)`: the directory, which on Linux is a
/// pidfd as well; `None` when it will not open -- the process has gone.
#[cfg(unix)]
fn open_dir(path: &Path) -> Option<ProcDir> {
    std::fs::File::open(path).ok().map(ProcDir::Open)
}

/// Windows opens no directory as a file; the host's tests only need to know
/// it is there.
#[cfg(not(unix))]
fn open_dir(path: &Path) -> Option<ProcDir> {
    path.is_dir().then_some(ProcDir::Present)
}

/// The descriptor of a `/proc/PID` directory, for `pidfd_send_signal`.
#[cfg(unix)]
fn raw_fd(file: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

/// No descriptors off Unix; nothing is sent there.
#[cfg(not(unix))]
fn raw_fd(_file: &std::fs::File) -> i32 {
    -1
}

/// The options upstream keeps in globals.
#[derive(Default)]
struct Opts {
    verbose: bool,
    exact: bool,
    interactive: bool,
    reg: bool,
    quiet: bool,
    wait_until_dead: bool,
    process_group: bool,
    ignore_case: bool,
    younger_than: i64,
    older_than: i64,
    ns_pid: i32,
}

/// One NAME, as `build_nameinfo` keeps it: its length, and for a path the
/// file it names.
struct NameInfo {
    name: Vec<u8>,
    name_length: usize,
    /// `(st_dev, st_ino)` of a NAME with a `/`; `None` for a command name.
    file: Option<(u64, u64)>,
}

/// The program, over the `/proc` at `proc_root`.
struct Killall<'s> {
    proc_root: PathBuf,
    sys: &'s dyn System,
    opts: Opts,
    utf8: bool,
}

/// `fprintf (stderr, ...)`: one complaint, unchecked as upstream's are.
fn complain(text: &[u8]) {
    stdfd::diag_bytes(text);
}

/// `usage (msg)`: the message, if any, then the text, on standard error;
/// status 1.
fn usage(msg: Option<&str>) -> Exit {
    let mut text = Vec::new();
    if let Some(msg) = msg {
        text.extend_from_slice(msg.as_bytes());
        text.push(b'\n');
    }
    text.extend_from_slice(
        b"Usage: killall [OPTION]... [--] NAME...\n       \
          killall -l, --list\n       \
          killall -V, --version\n\n  \
          -e,--exact          require exact match for very long names\n  \
          -I,--ignore-case    case insensitive process name match\n  \
          -g,--process-group  kill process group instead of process\n  \
          -y,--younger-than   kill processes younger than TIME\n  \
          -o,--older-than     kill processes older than TIME\n  \
          -i,--interactive    ask for confirmation before killing\n  \
          -l,--list           list all known signal names\n  \
          -q,--quiet          don't print complaints\n  \
          -r,--regexp         interpret NAME as an extended regular expression\n  \
          -s,--signal SIGNAL  send this signal instead of SIGTERM\n  \
          -u,--user USER      kill only process(es) running as USER\n  \
          -v,--verbose        report if the signal was successfully sent\n  \
          -V,--version        display version information\n  \
          -w,--wait           wait for processes to die\n  \
          -n,--ns PID         match processes that belong to the same namespaces\n                      \
          as PID\n  \
          -Z,--context REGEXP kill only process(es) having context\n                      \
          (must precede other arguments)\n\n",
    );
    complain(&text);
    Exit(1)
}

/// `list_signals ()`: the names, a blank between them, a line broken before
/// one that would pass column 80.
fn list_signals() -> Vec<u8> {
    let mut out = Vec::new();
    let mut col = 0usize;
    for (_, name) in SIGNALS {
        if col.saturating_add(name.len()).saturating_add(1) > 80 {
            out.push(b'\n');
            col = 0;
        }
        if col != 0 {
            out.push(b' ');
        }
        out.extend_from_slice(name.as_bytes());
        col = col.saturating_add(name.len()).saturating_add(1);
    }
    out.push(b'\n');
    out
}

/// C's `atoi`: leading blanks, a sign, digits, the rest ignored; a number
/// past `int` is cut as `strtol`'s `long` is cut to `int`.
fn atoi(text: &[u8]) -> i32 {
    coreutils::procps::scanf::atoi(text)
}

/// `get_signal (name, "killall")`: a number when the word begins with a
/// digit, else the table's name, `SIG` allowed in front. An unknown name ends
/// the run.
fn get_signal(name: &[u8]) -> Result<i32, Exit> {
    if name.first().is_some_and(u8::is_ascii_digit) {
        return Ok(atoi(name));
    }
    let bare = name.strip_prefix(b"SIG").unwrap_or(name);
    if let Some(&(num, _)) = SIGNALS.iter().find(|(_, n)| n.as_bytes() == bare) {
        return Ok(num);
    }
    let mut text = bare.to_vec();
    text.extend_from_slice(b": unknown signal; killall -l lists signals.\n");
    complain(&text);
    Err(Exit(1))
}

/// `parse_time_units (age)`: a number and a unit -- `s`, `m`, `h`, `d`, `w`,
/// `M` (four weeks), `y` (48 weeks) -- in seconds; -1 for anything else.
fn parse_time_units(age: &[u8]) -> i64 {
    let (num, used) = coreutils::procps::scanf::strtol(age);
    if used == 0 {
        return -1;
    }
    let Some(&unit) = age.get(used) else {
        return -1;
    };
    // `long` arithmetic, which wraps where C's would overflow.
    let times = |k: i64| num.wrapping_mul(k);
    match unit {
        b's' => num,
        b'm' => times(60),
        b'h' => times(60 * 60),
        b'd' => times(60 * 60 * 24),
        b'w' => times(60 * 60 * 24 * 7),
        b'M' => times(60 * 60 * 24 * 7 * 4),
        b'y' => times(60 * 60 * 24 * 7 * 4 * 12),
        _ => -1,
    }
}

/// `strcmp2`/`strncmp2`: whether `a` and `b` agree in their first `len` bytes
/// -- all of them when `len` is `None` -- folding ASCII case when asked, as
/// `strcasecmp` does in the C locale and, for these names, in any other.
fn same(a: &[u8], b: &[u8], len: Option<usize>, fold: bool) -> bool {
    let (a, b) = match len {
        Some(n) => (
            a.get(..n.min(a.len())).unwrap_or(a),
            b.get(..n.min(b.len())).unwrap_or(b),
        ),
        None => (a, b),
    };
    if fold {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// `match_process_name`: a command NAME against a process's name, or its
/// long name from `cmdline` when there was one.
fn match_process_name(
    comm: &[u8],
    cmdline: Option<&[u8]>,
    name: &[u8],
    name_length: usize,
    fold: bool,
) -> bool {
    let long = cmdline.unwrap_or_default();
    for cut in [OLD_COMM_LEN - 1, COMM_LEN - 1] {
        if comm.len() == cut && name_length >= cut {
            return if cmdline.is_some() {
                same(name, long, None, fold)
            } else {
                same(name, comm, Some(cut), fold)
            };
        }
    }
    if cmdline.is_some() {
        return same(name, long, None, fold);
    }
    same(name, comm, None, fold)
}

impl Killall<'_> {
    /// A file of process `pid`'s directory.
    fn proc_file(&self, pid: i32, file: &str) -> PathBuf {
        self.proc_root.join(pid.to_string()).join(file)
    }

    /// `uptime ()`: `/proc/uptime`'s first number. A missing file ends the
    /// run, as upstream's `exit (1)` does.
    fn uptime(&self) -> Result<f64, Exit> {
        let Ok(text) = std::fs::read(self.proc_root.join("uptime")) else {
            complain(b"killall: error opening uptime file\n");
            return Err(Exit(1));
        };
        let word: Vec<u8> = text
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .take_while(|b| !b.is_ascii_whitespace())
            .take(2047)
            .collect();
        Ok(atof(&word))
    }

    /// `process_age (jf)`: seconds since a process started, from its start
    /// time in clock ticks; never below 0.
    fn process_age(&self, jiffies: u64) -> Result<f64, Exit> {
        // The precision a `double` has, as upstream's division has it.
        #[allow(clippy::cast_precision_loss)]
        let started = jiffies as f64 / self.sys.clk_tck();
        let age = self.uptime()? - started;
        Ok(if age < 0.0 { 0.0 } else { age })
    }

    /// `load_process_name_and_age`: the process's name from `stat`, cut to
    /// 63 bytes, and with `load_age` its age. `None` for a process whose
    /// `stat` will not say -- skipped, as upstream skips it.
    fn load_process_name_and_age(
        &self,
        pid: i32,
        load_age: bool,
    ) -> Result<Option<(Vec<u8>, f64)>, Exit> {
        let Ok(text) = std::fs::read(self.proc_file(pid, "stat")) else {
            return Ok(None);
        };
        // `fgets (buf, 1024, file)`: the first line, at most 1023 bytes.
        let line = text.split(|&b| b == b'\n').next().unwrap_or_default();
        let buf = line.get(..line.len().min(1023)).unwrap_or(line);
        let Some(open) = buf.iter().position(|&b| b == b'(') else {
            return Ok(None);
        };
        let start = open.saturating_add(1);
        let Some(close) = buf
            .get(start..)
            .and_then(|rest| rest.iter().rposition(|&b| b == b')'))
        else {
            return Ok(None);
        };
        let full = buf
            .get(start..start.saturating_add(close))
            .unwrap_or_default();
        let comm = full
            .get(..full.len().min(COMM_LEN - 1))
            .unwrap_or(full)
            .to_vec();
        if !load_age {
            return Ok(Some((comm, 0.0)));
        }
        // `endcomm += 2`, then nineteen fields skipped and the start time.
        let after = buf
            .get(start.saturating_add(close).saturating_add(2)..)
            .unwrap_or_default();
        let fields: Vec<&[u8]> = after
            .split(u8::is_ascii_whitespace)
            .filter(|f| !f.is_empty())
            .collect();
        let Some(start_time) = fields.get(19) else {
            return Ok(None);
        };
        // `%Lu`: `strtoull`'s reading, which must convert something.
        let (jiffies, used, _) = cstrtol::strtoull(start_time, 10);
        if used == 0 {
            return Ok(None);
        }
        Ok(Some((comm, self.process_age(jiffies)?)))
    }

    /// `load_proc_cmdline`: the first argument whose last path component
    /// begins with the first `check_len` bytes of `comm` -- a long name the
    /// kernel cut. `None` when there is none (and, under `-e`, the process is
    /// skipped). `Err` for a `cmdline` that cannot be read: skipped too.
    fn load_proc_cmdline(
        &self,
        pid: i32,
        comm: &[u8],
        check_len: usize,
    ) -> Result<Option<Vec<u8>>, ()> {
        let text = std::fs::read(self.proc_file(pid, "cmdline")).map_err(|_| ())?;
        let mut found = None;
        for arg in text.split(|&b| b == 0) {
            if arg.is_empty() {
                break;
            }
            let base = match arg.iter().rposition(|&b| b == b'/') {
                Some(at) => arg.get(at.saturating_add(1)..).unwrap_or_default(),
                None => arg,
            };
            if same(base, comm, Some(check_len), false) {
                found = Some(base.to_vec());
                break;
            }
        }
        if self.opts.exact && found.is_none() {
            if self.opts.verbose {
                let mut text = b"killall: skipping partial match ".to_vec();
                text.extend_from_slice(comm);
                text.extend_from_slice(format!("({pid})\n").as_bytes());
                complain(&text);
            }
            return Err(());
        }
        Ok(found)
    }

    /// `match_process_uid`: whether the process's real uid is `uid`. A
    /// `status` with no `Uid:` line ends the run.
    fn match_process_uid(&self, pid: i32, uid: u32) -> Result<bool, Exit> {
        let Ok(text) = std::fs::read(self.proc_file(pid, "status")) else {
            return Ok(false);
        };
        for line in text.split(|&b| b == b'\n') {
            if let Some(rest) = line.strip_prefix(b"Uid:") {
                let (value, used) = coreutils::procps::scanf::strtol(rest);
                if used != 0 {
                    return Ok(u32::try_from(value).is_ok_and(|v| v == uid));
                }
            }
        }
        complain(b"killall: Cannot get UID from process status\n");
        Err(Exit(1))
    }

    /// `get_ns (pid, PIDNS)`: the inode of the process's PID namespace, as an
    /// `int`, or 0.
    fn pid_ns(&self, pid: i32) -> i32 {
        ns_inode(&self.proc_file(pid, "ns/pid"))
    }

    /// `match_process_context`: with SELinux not enabled -- every system this
    /// runs on -- the first line of the process's `attr/current` against the
    /// pattern; a process whose file cannot be read matches.
    fn match_process_context(&self, pid: i32, context: &Regex) -> Result<bool, Exit> {
        let Ok(text) = std::fs::read(self.proc_file(pid, "attr/current")) else {
            return Ok(true);
        };
        // `fgets (readbuf, BUFSIZ, file)`: its line, newline kept.
        let end = text
            .iter()
            .position(|&b| b == b'\n')
            .map_or(text.len(), |at| at.saturating_add(1));
        let line = text.get(..end.min(8191)).unwrap_or_default();
        if line.is_empty() {
            return Ok(true);
        }
        regexec(context, line)
    }
}

/// C's `atof`, for `/proc/uptime`'s `12345.67`.
fn atof(word: &[u8]) -> f64 {
    let text = std::str::from_utf8(word).unwrap_or_default();
    let end = text
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map_or(text.len(), |(i, _)| i);
    text.get(..end).unwrap_or_default().parse().unwrap_or(0.0)
}

/// The inode of a namespace link, cut to an `int` as upstream's `return
/// st.st_ino` cuts it; 0 when it cannot be read.
#[cfg(unix)]
fn ns_inode(path: &Path) -> i32 {
    use std::os::unix::fs::MetadataExt;
    // The low 32 bits, as C's conversion of `ino_t` to `int` keeps them.
    #[allow(clippy::cast_possible_truncation)]
    std::fs::metadata(path).map_or(0, |m| i32::from_ne_bytes((m.ino() as u32).to_ne_bytes()))
}

/// No namespaces off Unix.
#[cfg(not(unix))]
fn ns_inode(_path: &Path) -> i32 {
    0
}

/// `(st_dev, st_ino)` of a file.
#[cfg(unix)]
fn dev_ino(meta: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

/// No inode numbers off Unix; two files never compare equal there.
#[cfg(not(unix))]
fn dev_ino(_meta: &std::fs::Metadata) -> (u64, u64) {
    (0, 0)
}

/// `regexec (re, text, 0, NULL, 0) == 0`, with the engine's budget
/// reported as a run-ending error (deliberate difference 5).
fn regexec(re: &Regex, text: &[u8]) -> Result<bool, Exit> {
    re.is_match(text).map_err(|limit| {
        complain(format!("killall: regex error: {limit}\n").as_bytes());
        Exit(1)
    })
}

/// `strerror (errno)`.
fn strerror(errno: i32) -> String {
    coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// `1UL << i`: NAME `i`'s bit; `MAX_NAMES` keeps `i` below 64.
fn bit(i: usize) -> u64 {
    u32::try_from(i)
        .ok()
        .and_then(|i| 1u64.checked_shl(i))
        .unwrap_or(0)
}

/// `ask (name, pid, signal)`: the question on standard output, an answer
/// from standard input -- `y` or `n` in front, as `rpmatch` reads it in the
/// C locale; anything else asks again; an empty line or the end of input is
/// no.
fn ask(
    out: &mut dyn Write,
    input: &mut dyn BufRead,
    name: &[u8],
    pid: i32,
    signal: i32,
    process_group: bool,
) -> bool {
    loop {
        let mut q = if signal == SIGTERM {
            b"Kill ".to_vec()
        } else {
            b"Signal ".to_vec()
        };
        q.extend_from_slice(name);
        q.extend_from_slice(
            format!(
                "({}{pid}) ? (y/N) ",
                if process_group { "pgid " } else { "" }
            )
            .as_bytes(),
        );
        // The stream keeps a failure for the close (deliberate difference 4).
        let _ = out.write_all(&q);
        let _ = out.flush();
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        match line.first() {
            Some(b'\n') => return false,
            Some(b'y' | b'Y') => return true,
            Some(b'n' | b'N') => return false,
            _ => {}
        }
    }
}

impl Killall<'_> {
    /// `kill_all (signal, names, pwent, scontext)`: the status.
    #[allow(clippy::too_many_lines)] // Upstream's one function, kept whole.
    fn kill_all(
        &self,
        signal: i32,
        names: &[Vec<u8>],
        uid: Option<u32>,
        context: Option<&Regex>,
        out: &mut dyn Write,
        input: &mut dyn BufRead,
    ) -> Result<u8, Exit> {
        let o = &self.opts;
        let ns_ino = if o.ns_pid != 0 {
            self.pid_ns(o.ns_pid)
        } else {
            0
        };

        let mut reglist: Vec<Regex> = Vec::new();
        let mut name_info: Vec<NameInfo> = Vec::new();
        if !names.is_empty() && o.reg {
            for name in names {
                let compiled = if self.utf8 {
                    Regex::new_syntax(name, o.ignore_case, Syntax::POSIX_EXTENDED)
                } else {
                    Regex::new_syntax_bytes(name, o.ignore_case, Syntax::POSIX_EXTENDED)
                };
                match compiled {
                    Ok(re) => reglist.push(re),
                    Err(_) => {
                        let mut text = b"killall: Bad regular expression: ".to_vec();
                        text.extend_from_slice(name);
                        text.push(b'\n');
                        complain(&text);
                        return Err(Exit(1));
                    }
                }
            }
        } else {
            for name in names {
                let file = if name.contains(&b'/') {
                    match std::fs::metadata(coreutils::quote::os_from_bytes(name)) {
                        Ok(meta) => Some(dev_ino(&meta)),
                        Err(e) => {
                            // `perror (name)`.
                            let mut text = name.clone();
                            text.extend_from_slice(b": ");
                            text.extend_from_slice(coreutils::errmsg::strerror(&e).as_bytes());
                            text.push(b'\n');
                            complain(&text);
                            return Err(Exit(1));
                        }
                    }
                } else {
                    None
                };
                name_info.push(NameInfo {
                    name: name.clone(),
                    name_length: name.len(),
                    file,
                });
            }
        }

        let pid_table = self.create_pid_table()?;
        let mut found: u64 = 0;
        let mut pid_killed: Vec<i32> = Vec::new();
        // `calloc (pids, sizeof (pid_t))`: zero for every process not (yet)
        // matched, which a matched one in group 0 then counts as seen.
        let mut pgids: Vec<i32> = vec![0; pid_table.len()];
        for (i, &pid) in pid_table.iter().enumerate() {
            // `open (pidpath, O_RDONLY|O_DIRECTORY)`.
            let Some(dir) = open_dir(&self.proc_root.join(pid.to_string())) else {
                continue;
            };
            if let Some(uid) = uid
                && !self.match_process_uid(pid, uid)?
            {
                continue;
            }
            if o.ns_pid != 0 && ns_ino != 0 && ns_ino != self.pid_ns(pid) {
                continue;
            }
            if let Some(context) = context
                && !self.match_process_context(pid, context)?
            {
                continue;
            }
            let load_age = o.younger_than != 0 || o.older_than != 0;
            let Some((comm, age)) = self.load_process_name_and_age(pid, load_age)? else {
                continue;
            };
            // The `long` comparisons upstream makes against a `double`.
            #[allow(clippy::cast_precision_loss)]
            {
                if o.younger_than != 0 && age > o.younger_than as f64 {
                    continue;
                }
                if o.older_than != 0 && age < o.older_than as f64 {
                    continue;
                }
            }
            let mut command: Option<Vec<u8>> = None;
            if comm.len() == COMM_LEN - 1 || comm.len() == OLD_COMM_LEN - 1 {
                match self.load_proc_cmdline(pid, &comm, comm.len()) {
                    Ok(long) => command = long,
                    Err(()) => continue,
                }
            }

            let mut found_name: Option<usize> = None;
            for j in 0..names.len() {
                let hit = if o.reg {
                    let Some(re) = reglist.get(j) else { continue };
                    regexec(re, command.as_deref().unwrap_or(&comm))?
                } else {
                    let Some(info) = name_info.get(j) else {
                        continue;
                    };
                    match info.file {
                        None => match_process_name(
                            &comm,
                            command.as_deref(),
                            &info.name,
                            info.name_length,
                            o.ignore_case,
                        ),
                        Some(want) => self.same_exe(pid, &info.name, want),
                    }
                };
                if hit {
                    found_name = Some(j);
                    break;
                }
            }
            if !names.is_empty() && found_name.is_none() {
                continue;
            }

            let shown = command.as_deref().unwrap_or(&comm);
            let id = if o.process_group {
                let id = match self.sys.getpgid(pid) {
                    Ok(pgid) => pgid,
                    Err(errno) => {
                        complain(
                            format!("killall: getpgid({pid}): {}\n", strerror(errno)).as_bytes(),
                        );
                        -1
                    }
                };
                if let Some(slot) = pgids.get_mut(i) {
                    *slot = id;
                }
                if pgids.get(..i).is_some_and(|before| before.contains(&id)) {
                    continue;
                }
                // Deliberate difference 2: a group that could not be read
                // is no group to signal.
                if id < 0 {
                    continue;
                }
                id
            } else {
                pid
            };
            if o.interactive && !ask(out, input, &comm, id, signal, o.process_group) {
                continue;
            }
            let target = if o.process_group {
                id.wrapping_neg()
            } else {
                id
            };
            match self.sys.send(dir.file(), target, signal) {
                Ok(()) => {
                    if o.verbose {
                        let mut text = b"Killed ".to_vec();
                        text.extend_from_slice(shown);
                        text.extend_from_slice(
                            format!(
                                "({}{id}) with signal {signal}\n",
                                if o.process_group { "pgid " } else { "" }
                            )
                            .as_bytes(),
                        );
                        complain(&text);
                    }
                    if let Some(j) = found_name {
                        found |= bit(j);
                    }
                    pid_killed.push(id);
                }
                Err(errno) => {
                    if errno != ESRCH || o.interactive {
                        let mut text = shown.to_vec();
                        text.extend_from_slice(format!("({id}): {}\n", strerror(errno)).as_bytes());
                        complain(&text);
                    }
                }
            }
        }

        if !o.quiet {
            for (i, name) in names.iter().enumerate() {
                if found & bit(i) == 0 {
                    let mut text = name.clone();
                    text.extend_from_slice(b": no process found\n");
                    complain(&text);
                }
            }
        }
        let error = if names.is_empty() {
            u8::from(pid_killed.is_empty())
        } else {
            // Every NAME's bit set: `(1 << (n - 1)) | ((1 << (n - 1)) - 1)`,
            // which upstream spells so that 64 names do not shift past the
            // word.
            let top = bit(names.len().saturating_sub(1));
            let all = top | top.saturating_sub(1);
            u8::from(found != all)
        };
        while !pid_killed.is_empty() && o.wait_until_dead {
            pid_killed.retain(|&id| {
                !self.sys.gone(if o.process_group {
                    id.wrapping_neg()
                } else {
                    id
                })
            });
            self.sys.sleep_a_second();
        }
        Ok(error)
    }

    /// `create_pid_table`: every numbered directory of `/proc`, in
    /// directory order, this process left out.
    fn create_pid_table(&self) -> Result<Vec<i32>, Exit> {
        let entries = match std::fs::read_dir(&self.proc_root) {
            Ok(entries) => entries,
            Err(e) => {
                let mut text = b"/proc: ".to_vec();
                text.extend_from_slice(coreutils::errmsg::strerror(&e).as_bytes());
                text.push(b'\n');
                complain(&text);
                return Err(Exit(1));
            }
        };
        let me = self.sys.getpid();
        let mut table = Vec::new();
        for entry in entries.flatten() {
            let pid = atoi(&os_bytes(&entry.file_name()));
            if pid == 0 || pid == me {
                continue;
            }
            table.push(pid);
        }
        Ok(table)
    }

    /// A NAME with a `/` against the process's `exe`: the same file by device
    /// and inode, or -- the binary may have been replaced since -- a link
    /// whose text is the NAME.
    fn same_exe(&self, pid: i32, name: &[u8], want: (u64, u64)) -> bool {
        let exe = self.proc_file(pid, "exe");
        let Ok(meta) = std::fs::metadata(&exe) else {
            return false;
        };
        if dev_ino(&meta) == want {
            return true;
        }
        std::fs::read_link(&exe).is_ok_and(|link| os_bytes(link.as_os_str()).as_ref() == name)
    }

    /// `have_proc_self_stat ()`.
    fn have_proc_self_stat(&self) -> bool {
        self.proc_file(self.sys.getpid(), "stat").exists()
    }
}

/// The run: argv without its name, standard output and input.
#[allow(clippy::too_many_lines)] // Upstream's `main`, kept whole.
fn run(
    argv0: &[u8],
    words: &[OsString],
    k: &mut Killall<'_>,
    out: &mut dyn Write,
    input: &mut dyn BufRead,
) -> Result<u8, Exit> {
    if words.is_empty() {
        return Err(usage(None));
    }
    // C's `argv[i]`, `argv[0]` the program's name.
    let c_argv = |i: usize| -> Option<Vec<u8>> {
        if i == 0 {
            Some(argv0.to_vec())
        } else {
            words
                .get(i.saturating_sub(1))
                .map(|w| os_bytes(w).into_owned())
        }
    };
    let mut sig_num = SIGTERM;
    let mut uid: Option<u32> = None;
    let mut context: Option<Regex> = None;
    let mut skip_error: Option<usize> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut parser = KILLALL
        .parse(words, SHORTS, LONGS)
        .long_only(true)
        .keep_going(true);
    while let Some(item) = parser.next() {
        // glibc's `optind`, which counts `argv[0]`.
        let optind = parser.optind().saturating_add(1);
        let prev = c_argv(optind.saturating_sub(1)).unwrap_or_default();
        let opt = match item {
            Ok(opt) => opt,
            Err(_) => {
                if skip_error == Some(optind) {
                    continue;
                }
                // "Sigh, this is a hack because -ve could be -version or
                // -verbose" -- upstream's words.
                if prev.starts_with(b"-ve") {
                    k.opts.verbose = true;
                    k.opts.exact = true;
                    continue;
                }
                match prev.get(1) {
                    Some(c) if c.is_ascii_uppercase() => {
                        sig_num = get_signal(prev.get(1..).unwrap_or_default())?;
                    }
                    Some(c) if c.is_ascii_digit() => {
                        sig_num = atoi(prev.get(1..).unwrap_or_default());
                    }
                    _ => return Err(usage(None)),
                }
                continue;
            }
        };
        let value = |v: &Option<OsString>| {
            v.as_ref()
                .map(|v| os_bytes(v).into_owned())
                .unwrap_or_default()
        };
        match opt {
            Opt::Operand(word) => operands.push(os_bytes(word).into_owned()),
            Opt::Short(b'e', _) | Opt::Long("exact", _) => k.opts.exact = true,
            Opt::Short(b'g', _) | Opt::Long("process-group", _) => k.opts.process_group = true,
            Opt::Short(b'y', v) | Opt::Long("younger-than", v) => {
                let text = value(&v);
                let cut = text.get(..text.len().min(COMM_LEN - 1)).unwrap_or_default();
                k.opts.younger_than = parse_time_units(cut);
                if k.opts.younger_than <= 0 {
                    return Err(usage(Some("Invalid time format")));
                }
            }
            Opt::Short(b'o', v) | Opt::Long("older-than", v) => {
                let text = value(&v);
                let cut = text.get(..text.len().min(COMM_LEN - 1)).unwrap_or_default();
                k.opts.older_than = parse_time_units(cut);
                if k.opts.older_than <= 0 {
                    return Err(usage(Some("Invalid time format")));
                }
            }
            Opt::Short(b'i', _) | Opt::Long("interactive", _) => k.opts.interactive = true,
            Opt::Short(b'l', _) | Opt::Long("list-signals", _) => {
                // The stream keeps a failure for the close.
                let _ = out.write_all(&list_signals());
                return Ok(0);
            }
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => k.opts.quiet = true,
            Opt::Short(b'r', _) | Opt::Long("regexp", _) => k.opts.reg = true,
            Opt::Short(b's', v) | Opt::Long("signal", v) => sig_num = get_signal(&value(&v))?,
            Opt::Short(b'u', v) | Opt::Long("user", v) => {
                let name = value(&v);
                match pwdb::Db::load().user_by_name(&name) {
                    Some(user) => uid = Some(user.uid),
                    None => {
                        let mut text = b"Cannot find user ".to_vec();
                        text.extend_from_slice(&name);
                        text.push(b'\n');
                        complain(&text);
                        return Err(Exit(1));
                    }
                }
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => k.opts.verbose = true,
            Opt::Short(b'w', _) | Opt::Long("wait", _) => k.opts.wait_until_dead = true,
            Opt::Short(b'I', _) | Opt::Long("ignore-case", _) => {
                if prev == b"-I" || prev.starts_with(b"--") {
                    k.opts.ignore_case = true;
                } else {
                    // "option check is optind-1 but sig name is optind".
                    let next = c_argv(optind).unwrap_or_default();
                    sig_num = get_signal(next.get(1..).unwrap_or_default())?;
                    skip_error = Some(optind);
                }
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                if prev == b"-V" || prev.starts_with(b"--") {
                    complain(b"killall (SlateOS coreutils) 0.1.0\n");
                    return Ok(0);
                }
                let next = c_argv(optind).unwrap_or_default();
                sig_num = get_signal(next.get(1..).unwrap_or_default())?;
                skip_error = Some(optind);
            }
            Opt::Short(b'n', v) | Opt::Long("ns", v) => {
                // `errno != 0 || optarg == end`: out of `long`'s range, or no
                // digits; what follows them is not looked at.
                let text = value(&v);
                let (num, used, overflow) = cstrtol::strtol_overflow(&text, 10);
                if used == 0 || overflow {
                    return Err(usage(Some("Invalid namespace PID")));
                }
                k.opts.ns_pid = coreutils::procps::scanf::low_i32(num);
            }
            Opt::Short(b'Z', v) | Opt::Long("context", v) => {
                let text = value(&v);
                match Regex::new_syntax(&text, false, Syntax::POSIX_EXTENDED) {
                    Ok(re) => context = Some(re),
                    Err(_) => {
                        let mut msg = b"Bad regular expression: ".to_vec();
                        msg.extend_from_slice(&text);
                        msg.push(b'\n');
                        complain(&msg);
                        return Err(Exit(1));
                    }
                }
            }
            // Unreachable: every option in the tables has its case.
            Opt::Short(..) | Opt::Long(..) => return Err(usage(None)),
        }
    }
    if operands.is_empty() && uid.is_none() && context.is_none() {
        return Err(usage(None));
    }
    if operands.len() > MAX_NAMES {
        complain(format!("killall: Maximum number of names is {MAX_NAMES}\n").as_bytes());
        return Err(Exit(1));
    }
    if !k.have_proc_self_stat() {
        complain(b"killall: /proc lacks process entries (not mounted ?)\n");
        return Err(Exit(1));
    }
    k.kill_all(sig_num, &operands, uid, context.as_ref(), out, input)
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let sys = Libc;
    let mut k = Killall {
        proc_root: PathBuf::from("/proc"),
        sys: &sys,
        opts: Opts::default(),
        utf8: coreutils::locale::ctype_is_utf8(),
    };
    let mut out = Stream::stdout();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let status = match run(
        &argv0,
        argv.get(1..).unwrap_or_default(),
        &mut k,
        &mut out,
        &mut input,
    ) {
        Ok(status) | Err(Exit(status)) => status,
    };
    stdfd::close_stdout_bytes(b"killall", out, ExitCode::from(status), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// One send: the PID (a group when negative) and the signal.
    type Sent = (i32, i32);

    /// What a run did: its status, what it sent and what it wrote.
    type Ran = (u8, Vec<Sent>, Vec<u8>);

    /// A system that records sends and signals nothing; every process is in
    /// the group its fixture names, and is gone once signalled.
    struct Fake {
        sent: RefCell<Vec<Sent>>,
        groups: Vec<(i32, i32)>,
    }

    impl System for Fake {
        fn send(&self, _dir: Option<&std::fs::File>, pid: i32, sig: i32) -> Result<(), i32> {
            self.sent.borrow_mut().push((pid, sig));
            Ok(())
        }
        fn gone(&self, _pid: i32) -> bool {
            true
        }
        fn getpgid(&self, pid: i32) -> Result<i32, i32> {
            self.groups
                .iter()
                .find(|(p, _)| *p == pid)
                .map(|&(_, g)| g)
                .ok_or(ESRCH)
        }
        fn getpid(&self) -> i32 {
            999
        }
        fn clk_tck(&self) -> f64 {
            100.0
        }
        fn sleep_a_second(&self) {}
    }

    /// A process for the fixture: its PID, the name in `stat`, its start
    /// time in ticks, its `cmdline` and its uid.
    struct P(i32, &'static str, u64, &'static [u8], u32);

    /// A `/proc` holding `procs`, this process (999) and an uptime of 1000 s.
    fn world(procs: &[P]) -> scratchdir::ScratchDir {
        let dir = scratchdir::ScratchDir::new("killall");
        std::fs::write(dir.path("uptime"), "1000.00 3000.00\n").unwrap();
        let me = P(999, "killall", 0, b"killall\0", 0);
        for &P(pid, name, start, cmdline, uid) in procs.iter().chain([&me]) {
            let d = dir.path(&pid.to_string());
            std::fs::create_dir(&d).unwrap();
            // State, then fields 4 to 21, then the start time and two more.
            let middle: Vec<String> = (4..=21).map(|i| i.to_string()).collect();
            std::fs::write(
                d.join("stat"),
                format!("{pid} ({name}) S {} {start} 0 0\n", middle.join(" ")),
            )
            .unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
            std::fs::write(
                d.join("status"),
                format!("Name:\t{name}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
        }
        dir
    }

    /// Run `words` over `procs`.
    fn go(procs: &[P], words: &[&str]) -> Ran {
        go_with(procs, words, &[], b"")
    }

    /// Run `words` over `procs`, with process groups and an input.
    fn go_with(procs: &[P], words: &[&str], groups: &[(i32, i32)], input: &[u8]) -> Ran {
        let root = world(procs);
        let sys = Fake {
            sent: RefCell::new(Vec::new()),
            groups: groups.to_vec(),
        };
        let mut k = Killall {
            proc_root: root.dir().to_path_buf(),
            sys: &sys,
            opts: Opts::default(),
            utf8: true,
        };
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let mut out = Vec::new();
        let mut input = input;
        let status = match run(b"killall", &words, &mut k, &mut out, &mut input) {
            Ok(s) | Err(Exit(s)) => s,
        };
        let sent = sys.sent.borrow().clone();
        (status, sent, out)
    }

    const SLEEP: P = P(200, "sleep", 50_000, b"sleep\x001000\0", 1000);
    const BASH: P = P(100, "bash", 10_000, b"-bash\0", 1000);
    const LONG: P = P(
        300,
        "averyveryverylo",
        90_000,
        b"/usr/bin/averyveryverylongname\0arg\0",
        0,
    );

    #[test]
    fn a_name_is_signalled_with_sigterm_and_the_status_is_zero() {
        let (status, sent, _) = go(&[BASH, SLEEP], &["sleep"]);
        assert_eq!((status, sent), (0, vec![(200, 15)]));
    }

    #[test]
    fn a_name_nothing_matches_is_status_one() {
        let (status, sent, _) = go(&[BASH, SLEEP], &["sleep", "nosuch"]);
        assert_eq!((status, sent), (1, vec![(200, 15)]));
    }

    #[test]
    fn signal_words_are_read_as_upstream_reads_them() {
        assert_eq!(go(&[SLEEP], &["-HUP", "sleep"]).1, vec![(200, 1)]);
        assert_eq!(go(&[SLEEP], &["-9", "sleep"]).1, vec![(200, 9)]);
        assert_eq!(go(&[SLEEP], &["-s", "USR1", "sleep"]).1, vec![(200, 10)]);
        assert_eq!(go(&[SLEEP], &["-SIGKILL", "sleep"]).1, vec![(200, 9)]);
        // `-INT` reaches `-I` as a bundle. Half way through one, glibc's
        // `optind` still names the word itself, so `argv[optind] + 1` is
        // `INT`: the signal, as upstream's hack means it to be.
        assert_eq!(go(&[SLEEP], &["-INT", "sleep"]).1, vec![(200, 2)]);
        assert_eq!(go(&[SLEEP], &["-VTALRM", "sleep"]).1, vec![(200, 26)]);
        // Alone, `-I` folds case.
        assert_eq!(go(&[SLEEP], &["-I", "SLEEP"]).1, vec![(200, 15)]);
        // An unknown name ends the run.
        assert_eq!(go(&[SLEEP], &["-s", "hup", "sleep"]), (1, vec![], vec![]));
    }

    #[test]
    fn a_long_name_is_confirmed_from_cmdline() {
        let (status, sent, _) = go(&[LONG], &["averyveryverylongname"]);
        assert_eq!((status, sent), (0, vec![(300, 15)]));
        // The cut name alone does not match a NAME that is longer.
        assert_eq!(go(&[LONG], &["averyveryverylongnamX"]).0, 1);
    }

    #[test]
    fn regular_expressions_and_case() {
        assert_eq!(go(&[BASH, SLEEP], &["-r", "^sl"]).1, vec![(200, 15)]);
        assert_eq!(go(&[BASH, SLEEP], &["-I", "SLEEP"]).1, vec![(200, 15)]);
        assert_eq!(go(&[BASH, SLEEP], &["SLEEP"]).0, 1);
        assert_eq!(
            go(&[SLEEP], &["-r", "("]).0,
            1,
            "a bad expression ends the run"
        );
    }

    #[test]
    fn user_and_age_narrow_the_table() {
        // Uptime 1000 s; `sleep` started at 500 s, `bash` at 100 s.
        assert_eq!(
            go(&[BASH, SLEEP], &["-y", "10m", "sleep", "bash"]).1,
            vec![(200, 15)]
        );
        assert_eq!(
            go(&[BASH, SLEEP], &["-o", "10m", "sleep", "bash"]).1,
            vec![(100, 15)]
        );
        assert_eq!(
            go(&[BASH, SLEEP], &["-y", "10"]).0,
            1,
            "no unit is the usage"
        );
    }

    #[test]
    fn a_group_is_signalled_once() {
        let groups = [(100, 100), (200, 100)];
        let (status, sent, _) = go_with(&[BASH, SLEEP], &["-g", "sleep", "bash"], &groups, b"");
        assert_eq!(sent, vec![(-100, 15)]);
        assert_eq!(status, 1, "the second name's process was in the same group");
    }

    #[test]
    fn interactive_asks_and_reads_the_answer() {
        let (_, sent, out) = go_with(&[BASH, SLEEP], &["-i", "sleep"], &[], b"x\ny\n");
        assert_eq!(sent, vec![(200, 15)]);
        assert_eq!(out, b"Kill sleep(200) ? (y/N) Kill sleep(200) ? (y/N) ");
        let (_, sent, _) = go_with(&[SLEEP], &["-i", "-HUP", "sleep"], &[], b"\n");
        assert!(sent.is_empty());
    }

    #[test]
    fn list_and_usage() {
        let (status, sent, out) = go(&[], &["-l"]);
        assert_eq!((status, sent.len()), (0, 0));
        assert_eq!(
            out,
            b"HUP INT QUIT ILL TRAP ABRT BUS FPE KILL USR1 SEGV USR2 PIPE ALRM TERM STKFLT\n\
              CHLD CONT STOP TSTP TTIN TTOU URG XCPU XFSZ VTALRM PROF WINCH POLL PWR SYS\n"
        );
        assert_eq!(go(&[], &[]).0, 1);
        assert_eq!(go(&[], &["-e"]).0, 1, "an option and no name");
    }

    #[test]
    fn time_units() {
        assert_eq!(parse_time_units(b"1h"), 3600);
        assert_eq!(parse_time_units(b"2d"), 172_800);
        assert_eq!(parse_time_units(b"1M"), 2_419_200);
        assert_eq!(parse_time_units(b"1y"), 29_030_400);
        assert_eq!(parse_time_units(b"5"), -1);
        assert_eq!(parse_time_units(b"s"), -1);
        assert_eq!(parse_time_units(b"5x"), -1);
    }

    #[test]
    fn get_signal_numbers_and_names() {
        assert_eq!(get_signal(b"HUP").ok(), Some(1));
        assert_eq!(get_signal(b"SIGWINCH").ok(), Some(28));
        assert_eq!(get_signal(b"9x").ok(), Some(9));
        assert!(get_signal(b"hup").is_err());
        assert!(get_signal(b"IOT").is_err());
        assert!(get_signal(b"").is_err());
    }
}
