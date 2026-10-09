//! sysvinit 3.08's `killall5` and `pidof`: one program, `src/killall5.c`,
//! which is `pidof` when started under that name and `killall5` under any
//! other. Ubuntu's sysvinit-utils 3.08-6ubuntu3 ships it unpatched, `pidof`
//! a link to `killall5`, so this is the program Ubuntu ships too;
//! `bin/killall5.rs` and `bin/pidof.rs` both run [`main`].
//!
//! # pidof
//!
//! The ids of the processes running each program named, highest first --
//! upstream builds its list by prepending each process `/proc` lists. A name
//! matches a process when it is the process's `argv[0]`, or `argv[0]`'s
//! last component is the name, or the name's last component is `argv[0]`;
//! and, for a name with a `/`, when it is the executable `/proc/PID/exe`
//! names (the real path, if there is a file there). `-x` lets a script's
//! `argv[1]` match too, when its last component is the command `stat` names.
//! A process whose `argv[0]` is empty or holds a blank, or a login shell
//! (`argv[0]` beginning with `-`, and no other argument), is matched by its
//! `stat` name. Zombies are left out unless `-z` asks for them.
//!
//! What upstream does and this keeps:
//!
//! - `getopt` is told to keep quiet, so an option that is not one, or lacks
//!   its argument, is the one sentence `invalid options on command line!`.
//! - `-s` gives one process for each name, not one in all. `-d` takes the
//!   first byte of its argument as the separator -- of an empty argument, a
//!   NUL. `-o` takes ids separated by `,`, `;` or `:`, `%PPID` for the
//!   parent; one that is not a number above 0 is complained about and the
//!   rest kept. `-c` is honoured only for root.
//! - `-n` and `PIDOF_NETFS` are accepted and change nothing: upstream builds
//!   its list of network filesystems, and the code that would read it can
//!   never run (`pidof`'s own `nfs` flag is a local always 0).
//! - What it has to say goes to syslog (`daemon.err`, the program's name and
//!   id) -- unless standard input is a terminal with a name, when it goes to
//!   standard error as `NAME: MESSAGE` and a newline, after the one most
//!   messages carry already.
//! - The status is 0 when a process was found, 1 when none was. Standard
//!   output is never checked: there is no `close_stdout`.
//!
//! # killall5
//!
//! `killall5 [-SIGNAL] [-o PID[,PID…]]`: every process but init, itself, its
//! own session and kernel threads is sent SIGNAL (9 when none is given) --
//! all of them stopped first with `kill (-1, SIGSTOP)` and continued
//! afterwards, unless `-o` named processes to leave out, when nothing is
//! stopped. The status is 0 when something was signalled, 2 when nothing
//! was, 1 when `/proc` could not be read. Kept: a signal is read from the
//! first argument whatever position it is given in, so `-o 1 -15` is the
//! usage; `/proc` is mounted, by running `mount`, when it is not there; and
//! the program ignores `SIGTERM` and locks itself into memory.
//!
//! # How it is laid out
//!
//! Upstream's `main`, `main_pidof` and `readproc` read globals and say what
//! they have to say as they go. Here each piece that decides something is a
//! function of what it reads -- [`parse_pidof`], [`parse_killall5`],
//! [`scan_procs`], [`pidof`], [`report`] -- handed a `say` for its
//! complaints, so that the tests can run every one of them without a real
//! `/proc`, a syslog or a signal sent; [`Killall5`] wires them to the real
//! ones in upstream's order.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use crate::getopt::{Opt, Program};
use crate::procps::scanf::{Scan, atoi};
use crate::quote::{os_bytes, os_from_bytes};
use crate::stdfd::{self, Stream};

/// `PATH_MAX`, upstream's buffer for `stat`, a `cmdline` argument and an
/// executable's path.
const PATH_MAX: usize = 4096;
/// `STATNAMELEN`: how much of a command `stat` keeps.
const STATNAMELEN: usize = 15;

/// `SIGKILL`, killall5's signal when none is given.
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
const SIGSTOP: i32 = 19;
const SIGCONT: i32 = 18;

/// One process, as `readproc` reads it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Proc {
    /// `/proc/PID/exe`'s target, when it could be read.
    pathname: Option<Vec<u8>>,
    /// The first argument, when it is not empty.
    argv0: Option<Vec<u8>>,
    /// The first later argument that does not begin with `-`, when there is
    /// one and it is not empty.
    argv1: Option<Vec<u8>>,
    /// The command `stat` gives, without its parentheses.
    statname: Vec<u8>,
    pid: i32,
    sid: i32,
    /// A kernel thread: no code addresses.
    kernel: bool,
}

/// The text after `s`'s last `/`, or all of it.
fn base(s: &[u8]) -> &[u8] {
    match s.iter().rposition(|&c| c == b'/') {
        Some(i) => s.get(i.saturating_add(1)..).unwrap_or_default(),
        None => s,
    }
}

impl Proc {
    fn argv0base(&self) -> Option<&[u8]> {
        self.argv0.as_deref().map(base)
    }

    fn argv1base(&self) -> Option<&[u8]> {
        self.argv1.as_deref().map(base)
    }
}

/// The bytes before a C string's NUL.
fn c_str(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b.get(..end).unwrap_or_default()
}

/// `strncmp (a, b, n) == 0` for C strings.
fn strneq(a: &[u8], b: &[u8], n: usize) -> bool {
    let a = c_str(a);
    let b = c_str(b);
    a.get(..n.min(a.len())) == b.get(..n.min(b.len()))
}

/// `readarg`: the next NUL-terminated argument of a `cmdline` file, at most
/// `PATH_MAX` bytes of it -- a longer one is read as two -- or `None` at the
/// end with nothing read.
fn readarg(data: &[u8], at: &mut usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut ended = true;
    while out.len() < PATH_MAX {
        match data.get(*at) {
            None => break,
            Some(&0) => {
                *at = at.saturating_add(1);
                ended = false;
                break;
            }
            Some(&c) => {
                out.push(c);
                *at = at.saturating_add(1);
                ended = false;
            }
        }
    }
    if ended && out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `"MESSAGE ("`, `token`, `")!\n"`: the complaint about one `-o` id.
fn illegal_omit(token: &[u8]) -> Vec<u8> {
    let mut m = b"illegal omit pid value (".to_vec();
    m.extend_from_slice(token);
    m.extend_from_slice(b")!\n");
    m
}

/// One `-o` argument: ids separated by `,`, `;` or `:` -- `strsep`'s fields,
/// so two separators together have an empty one between them -- each added
/// to `omit`, or, when it is not a number above 0, complained about to
/// `say`. `ppid` is what `%PPID` stands for, in pidof; killall5 knows no
/// `%PPID`, and reads it as the number it is not.
fn omit_list(list: &[u8], ppid: Option<i32>, omit: &mut Vec<i32>, say: &mut dyn FnMut(&[u8])) {
    for token in list.split(|&c| matches!(c, b',' | b';' | b':')) {
        let opid = match ppid {
            Some(ppid) if token == b"%PPID" => ppid,
            _ => atoi(token),
        };
        if opid < 1 {
            say(&illegal_omit(token));
            continue;
        }
        omit.push(opid);
    }
}

/// What `pidof`'s options asked for.
#[derive(Debug, PartialEq, Eq)]
struct PidofArgs {
    /// `-o`'s ids. Upstream's `PIDOF_OMIT` flag is set by any `-o`, but
    /// leaves out nothing unless an id was kept, so the list says it all.
    omit: Vec<i32>,
    /// `-s`.
    single: bool,
    /// `-q`.
    quiet: bool,
    /// `-c`, given by root: only processes with this one's root directory.
    chroot_check: bool,
    /// `-d`'s first byte; a blank when there is no `-d`.
    sep: u8,
    /// `-x`, counted.
    scripts_too: u32,
    /// `-z`.
    list_dz: bool,
    /// The names to look for, in order.
    names: Vec<Vec<u8>>,
}

/// How `pidof`'s options end.
#[derive(Debug, PartialEq, Eq)]
enum PidofOpts {
    /// Look for the names.
    Run(PidofArgs),
    /// `-h`: the usage, and status 0.
    Help,
    /// An option that is not one, or lacks its argument: status 1.
    Invalid,
}

/// The parser for `pidof`'s `getopt`, whose complaints are not printed.
const PIDOF: Program = Program::new("pidof", 1);

/// `main_pidof`'s `getopt` loop over `words`, the arguments after the
/// program's name. `euid_root` is whether `-c` is honoured, and `ppid` what
/// `%PPID` stands for; each complaint about an `-o` id goes to `say` as it is
/// made, so those before an invalid option are made too.
fn parse_pidof(
    words: &[OsString],
    euid_root: bool,
    ppid: i32,
    say: &mut dyn FnMut(&[u8]),
) -> PidofOpts {
    let mut a = PidofArgs {
        omit: Vec::new(),
        single: false,
        quiet: false,
        chroot_check: false,
        sep: b' ',
        scripts_too: 0,
        list_dz: false,
        names: Vec::new(),
    };
    for item in PIDOF.parse(words, "qhco:d:sxzn", &[]) {
        match item {
            Ok(Opt::Short(b'c', _)) => a.chroot_check |= euid_root,
            // `pidof_usage ()`, then `exit (0)`: the options after it unread.
            Ok(Opt::Short(b'h', _)) => return PidofOpts::Help,
            Ok(Opt::Short(b'd', Some(v))) => a.sep = os_bytes(&v).first().copied().unwrap_or(0),
            Ok(Opt::Short(b'o', Some(v))) => omit_list(&os_bytes(&v), Some(ppid), &mut a.omit, say),
            Ok(Opt::Short(b'q', _)) => a.quiet = true,
            Ok(Opt::Short(b's', _)) => a.single = true,
            Ok(Opt::Short(b'x', _)) => a.scripts_too = a.scripts_too.saturating_add(1),
            Ok(Opt::Short(b'z', _)) => a.list_dz = true,
            // `-n`: see the module's docs.
            Ok(Opt::Short(b'n', _)) => {}
            Ok(Opt::Operand(o)) => a.names.push(os_bytes(o).into_owned()),
            Ok(_) | Err(_) => return PidofOpts::Invalid,
        }
    }
    PidofOpts::Run(a)
}

/// What killall5's arguments asked for.
#[derive(Debug, PartialEq, Eq)]
struct KillArgs {
    /// The signal to send.
    sig: i32,
    /// The ids `-o` named.
    omit: Vec<i32>,
}

/// `main`'s reading of killall5's arguments (`argv`, its name first), or
/// `None` for the usage. Each word loses one leading `-`, in place; one that
/// then begins with `o` takes the next word as ids to leave out, and any
/// other has the signal read again from `argv[1]` as it stands -- so the
/// first argument is the signal wherever the others are. Complaints about
/// ids go to `say` as they are made.
fn parse_killall5(argv: &[OsString], say: &mut dyn FnMut(&[u8])) -> Option<KillArgs> {
    let mut words: Vec<Vec<u8>> = argv.iter().map(|a| os_bytes(a).into_owned()).collect();
    let mut a = KillArgs {
        sig: SIGKILL,
        omit: Vec::new(),
    };
    let mut c = 1usize;
    while let Some(word) = words.get_mut(c) {
        if word.first() == Some(&b'-') {
            word.remove(0);
        }
        if word.first() == Some(&b'o') {
            c = c.saturating_add(1);
            omit_list(words.get(c)?, None, &mut a.omit, say);
        } else {
            a.sig = atoi(words.get(1).map_or(&[][..], Vec::as_slice));
            if a.sig <= 0 || a.sig > 31 {
                return None;
            }
        }
        c = c.saturating_add(1);
    }
    Some(a)
}

/// `readproc`'s walk: every process `dir` lists, last first; `None` when
/// `dir` could not be listed. `dir`'s entries are read through `dir` and each
/// executable's link through `exe_dir`, which `readproc` makes `.` -- having
/// moved into `/proc` -- and `/proc`, as upstream reaches them. A zombie is
/// kept only with `list_dz`. What cannot be read is complained about to
/// `say`, as upstream words it, except a process that has gone, which is
/// passed over in silence.
#[allow(clippy::too_many_lines)]
fn scan_procs(
    dir: &Path,
    exe_dir: &Path,
    list_dz: bool,
    say: &mut dyn FnMut(&[u8]),
) -> Option<Vec<Proc>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut plist: Vec<Proc> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { break };
        let name = os_bytes(&entry.file_name()).into_owned();
        let pid = atoi(&name);
        if pid == 0 {
            continue;
        }
        let mut p = Proc {
            pid,
            ..Proc::default()
        };
        let here = dir.join(os_from_bytes(&name));
        // How upstream names the file in what it says: relative to /proc.
        let mut stat_path = name.clone();
        stat_path.extend_from_slice(b"/stat");

        let Ok(mut f) = std::fs::File::open(here.join("stat")) else {
            // The process disappeared.
            continue;
        };
        let mut raw = Vec::with_capacity(PATH_MAX);
        // `fread` of PATH_MAX bytes: what a failed read leaves is what had
        // been read, and an empty buffer is complained about below.
        let _ = (&mut f)
            .take(u64::try_from(PATH_MAX).unwrap_or(4096))
            .read_to_end(&mut raw);
        let buf = c_str(&raw);
        if buf.is_empty() {
            let mut m = b"can't read from ".to_vec();
            m.extend_from_slice(&stat_path);
            m.push(b'\n');
            say(&m);
            continue;
        }
        let mut s = buf
            .iter()
            .position(|&c| c == b' ')
            .map_or(buf.len(), |i| i.saturating_add(1));
        let q;
        if buf.get(s) == Some(&b'(') {
            let Some(close) = buf.iter().rposition(|&c| c == b')') else {
                let mut m = b"can't get program name from /proc/".to_vec();
                m.extend_from_slice(&stat_path);
                m.push(b'\n');
                say(&m);
                continue;
            };
            q = close;
            s = s.saturating_add(1);
        } else {
            q = buf
                .iter()
                .skip(s)
                .position(|&c| c == b' ')
                .map_or(buf.len(), |i| s.saturating_add(i));
        }
        p.statname = if q >= s {
            buf.get(s..q).unwrap_or_default().to_vec()
        } else {
            // The NUL written at `q` lies before the name: `strcpy` reads
            // on to the buffer's end.
            buf.get(s..).unwrap_or_default().to_vec()
        };
        let mut rest = if q < buf.len() {
            q.saturating_add(1)
        } else {
            q
        };
        while buf.get(rest) == Some(&b' ') {
            rest = rest.saturating_add(1);
        }

        // `sscanf (q, "%10s %*d %*d %d …(19 skipped)… %lu %lu", …) != 4`.
        // A conversion that fails fails every later one with it.
        let mut sc = Scan::new(buf.get(rest..).unwrap_or_default());
        let state = sc.word(10).map(<[u8]>::to_vec);
        let _ = sc.skip_uint().and_then(|()| sc.skip_uint());
        let sid = sc.int();
        // tty_nr .. rsslim: two `%*d`, seven `%*u`, six `%*d`, two `%*u`, a
        // `%*d`, a `%*u`.
        for _ in 0..19 {
            let _ = sc.skip_uint();
        }
        let startcode = sc.ulong();
        let endcode = sc.ulong();
        let (Some(state), Some(sid), Some(startcode), Some(endcode)) =
            (state, sid, startcode, endcode)
        else {
            let mut m = b"can't read sid from ".to_vec();
            m.extend_from_slice(&stat_path);
            m.push(b'\n');
            say(&m);
            continue;
        };
        p.sid = sid;
        p.kernel = startcode == 0 && endcode == 0;
        if !list_dz && state.contains(&b'Z') {
            // A zombie.
            continue;
        }

        let Ok(cmdline) = std::fs::read(here.join("cmdline")) else {
            // The process disappeared.
            continue;
        };
        let mut at = 0usize;
        // An empty first argument, and none at all, are alike upstream: the
        // buffer it reads into holds an empty string either way.
        p.argv0 = readarg(&cmdline, &mut at).filter(|a| !a.is_empty());
        let mut arg: Vec<u8> = Vec::new();
        while let Some(a) = readarg(&cmdline, &mut at) {
            arg = a;
            if arg.first() != Some(&b'-') {
                break;
            }
            arg.clear();
        }
        if !arg.is_empty() {
            p.argv1 = Some(arg);
        }

        p.pathname = std::fs::read_link(exe_dir.join(os_from_bytes(&name)).join("exe"))
            .ok()
            .map(|t| {
                let mut b = os_bytes(t.as_os_str()).into_owned();
                b.truncate(PATH_MAX);
                b
            });
        plist.push(p);
    }
    // Each process was put at the front of the list.
    plist.reverse();
    Some(plist)
}

/// `pidof (prog)`: the indices into `plist` of the processes `prog` names,
/// in the list's order; `None` for a name ending in `/`. `root` is whether
/// the caller is root, who is never kept from an executable's link, and so
/// passes over a process whose link could not be read rather than match it
/// by name; `scripts_too` is `-x`.
fn pidof(plist: &[Proc], prog: &[u8], root: bool, scripts_too: bool) -> Option<Vec<usize>> {
    // A path to a file: its real path, to compare with the executable's.
    let real_path = if prog.first() == Some(&b'/') {
        std::fs::canonicalize(os_from_bytes(prog))
            .ok()
            .map(|p| os_bytes(p.as_os_str()).into_owned())
    } else {
        None
    };
    let s = base(prog);
    if s.is_empty() {
        return None;
    }
    let mut q = Vec::new();

    if let Some(real) = &real_path {
        for (i, p) in plist.iter().enumerate() {
            if p.pathname
                .as_deref()
                .is_some_and(|pn| c_str(pn) == real.as_slice())
                || p.argv0.as_deref() == Some(prog)
            {
                q.push(i);
            }
        }
        if !q.is_empty() {
            return Some(q);
        }
    }

    let short = s.len() <= STATNAMELEN;
    for (i, p) in plist.iter().enumerate() {
        if prog.first() == Some(&b'/') {
            let matched = match p.pathname.as_deref().map(c_str) {
                None => {
                    if root {
                        continue;
                    }
                    None
                }
                Some(pn) => {
                    // The program itself, or the program deleted.
                    if pn == prog
                        || (pn.starts_with(prog)
                            && pn.get(prog.len()..) == Some(&b" (deleted)"[..]))
                    {
                        Some(true)
                    } else if scripts_too {
                        None
                    } else {
                        Some(false)
                    }
                }
            };
            match matched {
                Some(true) => {
                    q.push(i);
                    continue;
                }
                Some(false) => continue,
                // On to the comparisons of names.
                None => {}
            }
        }

        let mut ok = p.argv0.as_deref() == Some(prog)
            || (s.len() != prog.len() && p.argv0.as_deref() == Some(s))
            || p.argv0base() == Some(prog);

        if scripts_too
            && p.argv1base()
                .is_some_and(|b1| strneq(&p.statname, b1, STATNAMELEN))
        {
            ok |= p.argv1.as_deref() == Some(prog)
                || (s.len() != prog.len() && p.argv1.as_deref() == Some(s))
                || p.argv1base() == Some(prog);
        }

        // A title set with `setproctitle`: try the stat name.
        if short
            && p.argv0
                .as_deref()
                .is_none_or(|a0| a0.is_empty() || a0.contains(&b' '))
        {
            ok |= p.statname == s;
        }

        // A login shell's `-` before its name.
        if short
            && p.argv1.is_none()
            && p.argv0
                .as_deref()
                .is_some_and(|a0| a0.first() == Some(&b'-'))
        {
            ok |= p.statname == s;
        }

        if ok {
            q.push(i);
        }
    }
    Some(q)
}

/// `main_pidof`'s loop over the names: the id of each process found, as
/// upstream prints them, to `out`; and whether any was. `root` is as for
/// [`pidof`]; `own_root` is the identity of this process's root directory,
/// under `-c`, which a process's must share to be printed.
fn report(
    plist: &[Proc],
    args: &PidofArgs,
    root: bool,
    own_root: Option<(u64, u64)>,
    out: &mut impl Write,
) -> bool {
    let mut first = true;
    for prog in &args.names {
        let Some(q) = pidof(plist, prog, root, args.scripts_too > 0) else {
            continue;
        };
        let mut spid = false;
        for i in q {
            let Some(p) = plist.get(i) else { continue };
            if args.omit.contains(&p.pid) {
                continue;
            }
            if args.single {
                if spid {
                    continue;
                }
                spid = true;
            }
            if let Some(own) = own_root
                && root_id(&format!("/proc/{}/root", p.pid)) != Some(own)
            {
                continue;
            }
            if !args.quiet {
                let mut line = Vec::new();
                if !first {
                    line.push(args.sep);
                }
                line.extend_from_slice(p.pid.to_string().as_bytes());
                // `printf`'s failure is not looked at; see the module docs.
                let _ = out.write_all(&line);
            }
            first = false;
        }
    }
    if !first && !args.quiet {
        let _ = out.write_all(b"\n");
    }
    !first
}

/// The program's one piece of state: its name, for what it says.
struct Killall5 {
    /// `progname`: what follows `argv[0]`'s last `/`.
    progname: Vec<u8>,
}

impl Killall5 {
    /// `nsyslog (LOG_ERR, …)`: to syslog, or -- standard input a terminal
    /// `ttyname` can name -- to standard error.
    fn nsyslog(&self, msg: &[u8]) {
        if stdin_has_a_tty_name() {
            let mut m = self.progname.clone();
            m.extend_from_slice(b": ");
            m.extend_from_slice(msg);
            m.push(b'\n');
            stdfd::diag_bytes_ahead_of_stdout(&m);
        } else {
            libcsyslog::syslog(libcsyslog::LOG_ERR, msg);
        }
    }

    /// `readproc`: every process `/proc` lists, last first; `None` when
    /// `/proc` could not be read at all. It works from inside `/proc`, as
    /// upstream does, so that a `/` the killing affects cannot hold it up.
    fn readproc(&self, list_dz: bool) -> Option<Vec<Proc>> {
        if std::env::set_current_dir("/proc").is_err() {
            self.nsyslog(b"chdir /proc failed");
            return None;
        }
        let plist = scan_procs(Path::new("."), Path::new("/proc"), list_dz, &mut |m| {
            self.nsyslog(m);
        });
        if plist.is_none() {
            self.nsyslog(b"cannot opendir(/proc)");
        }
        plist
    }

    /// `main_pidof`, and the status.
    fn main_pidof(&self, argv: &[OsString], out: &mut Stream) -> u8 {
        let words = argv.get(1..).unwrap_or(&[]);
        let euid_root = libcall::process::geteuid() == 0;
        let args = match parse_pidof(words, euid_root, parent_id(), &mut |m| self.nsyslog(m)) {
            PidofOpts::Run(args) => args,
            PidofOpts::Help => {
                let _ = out.write_all(PIDOF_USAGE.as_bytes());
                return 0;
            }
            PidofOpts::Invalid => {
                self.nsyslog(b"invalid options on command line!\n");
                libcsyslog::closelog();
                return 1;
            }
        };

        let own_root = if args.chroot_check {
            let me = format!("/proc/{}/root", std::process::id());
            let Some(id) = root_id(&me) else {
                let mut m = b"stat failed for ".to_vec();
                m.extend_from_slice(me.as_bytes());
                m.extend_from_slice(b"!\n");
                self.nsyslog(&m);
                libcsyslog::closelog();
                return 1;
            };
            Some(id)
        } else {
            None
        };

        // `readproc`'s failure leaves an empty list, as upstream ignores it.
        let plist = self.readproc(args.list_dz).unwrap_or_default();
        let root = libcall::process::getuid() == 0;
        let found = report(&plist, &args, root, own_root, out);
        libcsyslog::closelog();
        u8::from(!found)
    }

    /// `mount_proc`: `/proc` mounted, by running `mount`, when it is not
    /// there. `Err` is the status to exit with.
    fn mount_proc(&self) -> Result<(), u8> {
        if std::fs::metadata("/proc/version")
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            match run_mount(self) {
                // `WEXITSTATUS (wst) != 0`: a signal's status reads as 0.
                Some(status) if (status >> 8) & 0xff == 0 => {}
                Some(_) | None => self.nsyslog(b"mount returned non-zero exit status"),
            }
        }
        match std::fs::metadata("/proc/version") {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.nsyslog(b"/proc not mounted, failed to mount.");
                Err(1)
            }
            Err(_) => {
                self.nsyslog(b"/proc unavailable.");
                Err(1)
            }
        }
    }

    /// `main`'s killall5 half, and the status.
    fn main_killall5(&self, argv: &[OsString]) -> u8 {
        let Some(args) = parse_killall5(argv, &mut |m| self.nsyslog(m)) else {
            return self.usage();
        };

        if let Err(status) = self.mount_proc() {
            return status;
        }

        // Ignoring SIGKILL and SIGSTOP is refused, and harmless to ask.
        for s in [SIGTERM, SIGSTOP, SIGKILL] {
            let _ = libcall::ignore_signal(s);
        }
        // Locked into memory, so that nothing it needs is paged out while
        // everything else is stopped; refused to anyone but root, harmlessly.
        let _ = libcall::process::lock_all_memory();

        let sid = libcall::process::session_of(0).unwrap_or(-1);
        let pid = i32::try_from(std::process::id()).unwrap_or(-1);

        let sent_sigstop = args.omit.is_empty();
        if sent_sigstop {
            let _ = libcall::kill(-1, SIGSTOP);
        }
        let Some(plist) = self.readproc(false) else {
            if sent_sigstop {
                let _ = libcall::kill(-1, SIGCONT);
            }
            return 1;
        };
        let mut retval = 2;
        for p in &plist {
            if p.pid == 1 || p.pid == pid || p.sid == sid || p.kernel {
                continue;
            }
            if args.omit.contains(&p.pid) {
                continue;
            }
            // Not looked at upstream: a process that went is no failure.
            let _ = libcall::kill(p.pid, args.sig);
            retval = 0;
        }
        if sent_sigstop {
            let _ = libcall::kill(-1, SIGCONT);
        }
        libcsyslog::closelog();
        // `usleep (1)`: the scheduler given its turn.
        std::thread::sleep(std::time::Duration::from_micros(1));
        retval
    }

    /// `usage`: the one line, to syslog or standard error, and status 1.
    fn usage(&self) -> u8 {
        self.nsyslog(b"usage: killall5 -signum [-o omitpid] [-o omitpid] ...");
        libcsyslog::closelog();
        1
    }
}

/// `pidof_usage`.
const PIDOF_USAGE: &str = concat!(
    "pidof usage: [options] <program-name>\n",
    "\n",
    " -c           Return PIDs with the same root directory\n",
    " -d <sep>     Use the provided character as output separator\n",
    " -h           Display this help text\n",
    " -n           Avoid using stat system function on network shares\n",
    " -o <pid>     Omit results with a given PID\n",
    " -q           Quiet mode. Do not display output\n",
    " -s           Only return one PID\n",
    " -x           Return PIDs of shells running scripts with a matching name\n",
    " -z           List zombie and I/O waiting processes. May cause pidof to hang.\n",
    "\n",
);

/// `ttyname (0) != NULL`: standard input is a terminal, and one whose name
/// can be found -- glibc's `ttyname` gives up on a name of 4095 bytes or
/// more, which is the buffer this asks with.
fn stdin_has_a_tty_name() -> bool {
    let mut buf = [0u8; PATH_MAX];
    libcall::termios::ttyname_into(0, &mut buf).is_ok()
}

/// `(st_dev, st_ino)` of what `path` leads to.
#[cfg(unix)]
fn root_id(path: &str) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn root_id(_path: &str) -> Option<(u64, u64)> {
    None
}

/// `mount -t proc proc /proc` in a child, which tries `/bin/mount` and then
/// `/sbin/mount` and says so itself when it can run neither: the raw status
/// `wait` gave for it, or `None` when there was none to have.
///
/// The child's `argv[0]` is the path it runs, where upstream's is `mount`:
/// `libcall::process::execvp` takes the program from `argv[0]`. util-linux's
/// `mount` names itself by its last component either way.
#[cfg(unix)]
fn run_mount(k: &Killall5) -> Option<i32> {
    use libcall::process::Forked;
    // SAFETY: the program has one thread when it gets here; the child only
    // execs, or reports and leaves with `_exit`.
    match unsafe { libcall::process::fork() } {
        Err(_) => {
            k.nsyslog(b"cannot fork");
            libcsyslog::closelog();
            std::process::exit(1);
        }
        Ok(Forked::Child) => {
            for prog in [c"/bin/mount", c"/sbin/mount"] {
                let argv = [prog, c"-t", c"proc", c"proc", c"/proc"];
                let mut slots = [std::ptr::null(); 6];
                // Returns only when the exec failed; the next is tried.
                let _ = libcall::process::execvp(&argv, &mut slots);
            }
            k.nsyslog(b"cannot execute mount");
            libcall::process::exit_immediately(1);
        }
        Ok(Forked::Parent(pid)) => libcall::process::wait(pid).ok().map(|s| s.raw()),
    }
}

/// The host build has no `/proc` to mount.
#[cfg(not(unix))]
fn run_mount(k: &Killall5) -> Option<i32> {
    k.nsyslog(b"cannot fork");
    None
}

/// `getppid`.
#[cfg(unix)]
fn parent_id() -> i32 {
    i32::try_from(std::os::unix::process::parent_id()).unwrap_or(0)
}

#[cfg(not(unix))]
fn parent_id() -> i32 {
    0
}

/// The program, as `bin/killall5.rs` and `bin/pidof.rs` run it.
#[must_use]
pub fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let progname = base(&argv0).to_vec();
    libcsyslog::openlog(
        &progname,
        libcsyslog::LOG_CONS | libcsyslog::LOG_PID,
        libcsyslog::LOG_DAEMON,
    );
    let k = Killall5 { progname };
    let mut out = Stream::stdout();
    let status = if k.progname == b"pidof" {
        k.main_pidof(&argv, &mut out)
    } else {
        k.main_killall5(&argv)
    };
    // No `close_stdout`: what could not be written is not said, and the
    // status stands.
    let _ = out.finish();
    ExitCode::from(status)
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

    /// A process as `readproc` would have read it; `""` is "none".
    fn proc(pid: i32, argv0: &str, argv1: &str, statname: &str, pathname: &str) -> Proc {
        let some = |s: &str| (!s.is_empty()).then(|| s.as_bytes().to_vec());
        Proc {
            pathname: some(pathname),
            argv0: some(argv0),
            argv1: some(argv1),
            statname: statname.as_bytes().to_vec(),
            pid,
            sid: 0,
            kernel: false,
        }
    }

    /// The ids `pidof` finds for `prog`.
    fn found(plist: &[Proc], prog: &str, root: bool, scripts: bool) -> Option<Vec<i32>> {
        pidof(plist, prog.as_bytes(), root, scripts)
            .map(|q| q.iter().map(|&i| plist[i].pid).collect())
    }

    /// A directory no test host has, so that a path under it has no real
    /// path and is matched by the second pass.
    const NOWHERE: &str = "/nonexistent-killall5-test";

    #[test]
    fn arguments_are_read_up_to_their_nul_and_at_most_path_max() {
        let data = b"sleep\0-x\0\0arg\0";
        let mut at = 0;
        assert_eq!(readarg(data, &mut at), Some(b"sleep".to_vec()));
        assert_eq!(readarg(data, &mut at), Some(b"-x".to_vec()));
        assert_eq!(readarg(data, &mut at), Some(Vec::new()), "an empty one");
        assert_eq!(readarg(data, &mut at), Some(b"arg".to_vec()));
        assert_eq!(readarg(data, &mut at), None);
        let long = vec![b'a'; 5000];
        let mut at = 0;
        assert_eq!(readarg(&long, &mut at).map(|v| v.len()), Some(4096));
        assert_eq!(
            readarg(&long, &mut at).map(|v| v.len()),
            Some(904),
            "the rest"
        );
        assert_eq!(readarg(&long, &mut at), None);
        // The last one need not end in a NUL.
        let mut at = 0;
        assert_eq!(readarg(b"tail", &mut at), Some(b"tail".to_vec()));
        assert_eq!(readarg(b"tail", &mut at), None);
    }

    #[test]
    fn names_and_prefixes_as_c_compares_them() {
        assert_eq!(base(b"/usr/bin/sleep"), b"sleep");
        assert_eq!(base(b"sleep"), b"sleep");
        assert_eq!(base(b"dir/"), b"");
        assert!(strneq(b"bash", b"bash-extra", 4));
        assert!(!strneq(b"bash", b"bash-extra", 5));
        assert!(strneq(b"a", b"a", 15));
        assert!(strneq(b"abc\0x", b"abc\0y", 15), "both end at their NUL");
    }

    #[test]
    fn a_name_matches_argv0_its_last_part_or_the_last_part_of_the_name() {
        // Upstream's table: a process `b` matches b, p/b and q/b; a process
        // `p/b` matches b and p/b, not q/b.
        let plist = [proc(10, "b", "", "b", ""), proc(11, "p/b", "", "b", "")];
        assert_eq!(found(&plist, "b", true, false), Some(vec![10, 11]));
        assert_eq!(found(&plist, "p/b", true, false), Some(vec![10, 11]));
        assert_eq!(found(&plist, "q/b", true, false), Some(vec![10]));
        assert_eq!(found(&plist, "c", true, false), Some(vec![]));
    }

    #[test]
    fn a_name_ending_in_a_slash_names_nothing_at_all() {
        let plist = [proc(10, "b", "", "b", "")];
        assert_eq!(found(&plist, "b/", true, false), None);
        assert_eq!(found(&plist, "/", true, false), None);
        assert_eq!(found(&plist, "", true, false), None);
    }

    #[test]
    fn a_path_matches_the_executable_or_the_executable_deleted() {
        let p = format!("{NOWHERE}/bin/b");
        let plist = [
            proc(1, "b", "", "b", &p),
            proc(2, "b", "", "b", &format!("{p} (deleted)")),
            proc(3, "b", "", "b", "/elsewhere/b"),
            proc(4, &p, "", "b", ""),
            proc(5, "b", "", "b", &format!("{p}x")),
            proc(6, "b", "", "b", &format!("{p} (deleted) twice")),
        ];
        assert_eq!(found(&plist, &p, true, false), Some(vec![1, 2]));
        // Not root, an executable that could not be read is matched by name.
        assert_eq!(found(&plist, &p, false, false), Some(vec![1, 2, 4]));
        // -x: whatever is not the executable is compared by name too.
        assert_eq!(found(&plist, &p, true, true), Some(vec![1, 2, 3, 5, 6]));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_to_a_file_is_matched_by_its_real_path_before_anything_else() {
        let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let exe = exe.to_str().unwrap();
        let deleted = format!("{exe} (deleted)");
        let plist = [
            proc(1, "x", "", "x", exe),
            proc(2, exe, "", "x", "/elsewhere"),
            proc(3, "x", "", "x", &deleted),
        ];
        // The first pass found something, so the second is never made.
        assert_eq!(found(&plist, exe, true, false), Some(vec![1, 2]));
        let only_deleted = [proc(3, "x", "", "x", &deleted)];
        assert_eq!(found(&only_deleted, exe, true, false), Some(vec![3]));
    }

    #[test]
    fn a_title_with_a_blank_or_a_login_shell_is_matched_by_its_stat_name() {
        let plist = [
            proc(1, "gamma with spaces", "", "alpha", ""),
            proc(2, "-alpha", "", "alpha", ""),
            proc(3, "-alpha", "600", "alpha", ""),
            proc(4, "", "", "alpha", ""),
            proc(5, "plain", "", "alpha", ""),
        ];
        assert_eq!(found(&plist, "alpha", true, false), Some(vec![1, 2, 4]));
        // A whole title is an argv[0] like any other.
        assert_eq!(
            found(&plist, "gamma with spaces", true, false),
            Some(vec![1])
        );
        // Only a name `stat` could hold all of is compared with it.
        let names = [
            proc(1, "a b", "", "abcdefghijklmno", ""),
            proc(2, "a b", "", "abcdefghijklmnop", ""),
        ];
        assert_eq!(found(&names, "abcdefghijklmno", true, false), Some(vec![1]));
        assert_eq!(found(&names, "abcdefghijklmnop", true, false), Some(vec![]));
    }

    #[test]
    fn with_x_a_script_is_matched_by_its_argument_when_stat_names_it() {
        let script = format!("{NOWHERE}/myscript.sh");
        let plist = [
            proc(1, "/bin/sh", &script, "myscript.sh", "/usr/bin/dash"),
            // `stat` names the shell: not a script started through its `#!`.
            proc(
                2,
                "/bin/sh",
                "/usr/local/bin/other.sh",
                "sh",
                "/usr/bin/dash",
            ),
            proc(
                3,
                "/usr/bin/python3",
                "/opt/a-very-long-script-name.py",
                "a-very-long-scr",
                "/usr/bin/python3.12",
            ),
        ];
        assert_eq!(found(&plist, "myscript.sh", true, false), Some(vec![]));
        assert_eq!(found(&plist, "myscript.sh", true, true), Some(vec![1]));
        assert_eq!(found(&plist, &script, true, true), Some(vec![1]));
        assert_eq!(found(&plist, "other.sh", true, true), Some(vec![]));
        // `stat` keeps 15 bytes of a name, and only those are compared.
        assert_eq!(
            found(&plist, "a-very-long-script-name.py", true, true),
            Some(vec![3])
        );
        // The interpreter is found by its own name either way.
        assert_eq!(found(&plist, "sh", true, false), Some(vec![1, 2]));
    }

    /// `parse_pidof` over `args`, with what it said.
    fn pidof_opts(args: &[&str], euid_root: bool) -> (PidofOpts, Vec<Vec<u8>>) {
        let words: Vec<OsString> = args.iter().map(OsString::from).collect();
        let mut heard = Vec::new();
        let opts = parse_pidof(&words, euid_root, 77, &mut |m| heard.push(m.to_vec()));
        (opts, heard)
    }

    fn run(opts: PidofOpts) -> PidofArgs {
        match opts {
            PidofOpts::Run(a) => a,
            other => panic!("expected names to look for, got {other:?}"),
        }
    }

    #[test]
    fn pidof_reads_its_options() {
        let (opts, heard) =
            pidof_opts(&["-s", "-q", "-x", "-x", "-z", "-n", "alpha", "beta"], true);
        assert!(heard.is_empty());
        let a = run(opts);
        assert!(a.single && a.quiet && a.list_dz && !a.chroot_check);
        assert_eq!(a.scripts_too, 2);
        assert_eq!(a.sep, b' ');
        assert_eq!(a.names, [b"alpha".to_vec(), b"beta".to_vec()]);
        // Options after the names, as GNU getopt permutes them.
        let a = run(pidof_opts(&["alpha", "-s"], true).0);
        assert!(a.single);
        assert_eq!(a.names, [b"alpha".to_vec()]);
        // After `--`, an option is a name.
        let a = run(pidof_opts(&["--", "-s"], true).0);
        assert!(!a.single);
        assert_eq!(a.names, [b"-s".to_vec()]);
    }

    #[test]
    fn pidof_separates_with_the_first_byte_of_d_or_a_nul() {
        assert_eq!(run(pidof_opts(&["-d", ","], true).0).sep, b',');
        assert_eq!(run(pidof_opts(&["-dab"], true).0).sep, b'a');
        assert_eq!(run(pidof_opts(&["-d", ""], true).0).sep, 0);
    }

    #[test]
    fn pidof_honours_c_only_for_root() {
        assert!(run(pidof_opts(&["-c"], true).0).chroot_check);
        assert!(!run(pidof_opts(&["-c"], false).0).chroot_check);
    }

    #[test]
    fn pidof_leaves_out_what_o_lists_and_complains_of_the_rest() {
        let (opts, heard) = pidof_opts(&["-o", "2,3;4:5", "-o", "%PPID"], true);
        assert!(heard.is_empty());
        assert_eq!(run(opts).omit, [2, 3, 4, 5, 77]);
        let (opts, heard) = pidof_opts(&["-o", "x,0,-3,,7"], true);
        assert_eq!(run(opts).omit, [7]);
        assert_eq!(
            heard,
            [
                b"illegal omit pid value (x)!\n".to_vec(),
                b"illegal omit pid value (0)!\n".to_vec(),
                b"illegal omit pid value (-3)!\n".to_vec(),
                b"illegal omit pid value ()!\n".to_vec(),
            ]
        );
        let (opts, heard) = pidof_opts(&["-o", ""], true);
        assert_eq!(run(opts).omit, Vec::<i32>::new());
        assert_eq!(heard, [b"illegal omit pid value ()!\n".to_vec()]);
    }

    #[test]
    fn pidof_stops_at_h_or_at_an_option_that_is_none() {
        assert_eq!(pidof_opts(&["-h"], true).0, PidofOpts::Help);
        assert_eq!(pidof_opts(&["alpha", "-h", "-y"], true).0, PidofOpts::Help);
        assert_eq!(pidof_opts(&["-y", "-h"], true).0, PidofOpts::Invalid);
        assert_eq!(pidof_opts(&["alpha", "-o"], true).0, PidofOpts::Invalid);
        assert_eq!(pidof_opts(&["--help"], true).0, PidofOpts::Invalid);
        // What came before the invalid option was said all the same.
        let (opts, heard) = pidof_opts(&["-o", "x", "-y"], true);
        assert_eq!(opts, PidofOpts::Invalid);
        assert_eq!(heard, [b"illegal omit pid value (x)!\n".to_vec()]);
    }

    /// `parse_killall5` over `args` after the program's name, with what it
    /// said.
    fn kill_args(args: &[&str]) -> (Option<KillArgs>, Vec<Vec<u8>>) {
        let mut argv = vec![OsString::from("killall5")];
        argv.extend(args.iter().map(OsString::from));
        let mut heard = Vec::new();
        let a = parse_killall5(&argv, &mut |m| heard.push(m.to_vec()));
        (a, heard)
    }

    fn sig_of(args: &[&str]) -> Option<i32> {
        kill_args(args).0.map(|a| a.sig)
    }

    #[test]
    fn killall5_reads_its_signal_from_its_first_argument() {
        assert_eq!(
            kill_args(&[]).0,
            Some(KillArgs {
                sig: 9,
                omit: Vec::new()
            })
        );
        assert_eq!(sig_of(&["-15"]), Some(15));
        assert_eq!(sig_of(&["15"]), Some(15));
        assert_eq!(sig_of(&["-15x"]), Some(15));
        assert_eq!(sig_of(&["-1"]), Some(1));
        assert_eq!(sig_of(&["-31"]), Some(31));
        assert_eq!(sig_of(&["-0"]), None);
        assert_eq!(sig_of(&["-32"]), None);
        assert_eq!(sig_of(&["-x"]), None);
        assert_eq!(sig_of(&[""]), None);
        // One `-` comes off; the second leaves "-o", which is no number.
        assert_eq!(sig_of(&["--o"]), None);
        // Each later word reads the first again.
        assert_eq!(sig_of(&["-15", "-9"]), Some(15));
        // With `-o` first, the signal is read from the "o" it left.
        assert_eq!(sig_of(&["-o", "8", "-15"]), None);
    }

    #[test]
    fn killall5_leaves_out_what_o_lists() {
        let omit_of = |args: &[&str]| kill_args(args).0.map(|a| a.omit);
        assert_eq!(omit_of(&["-15", "-o", "2,3;4:5"]), Some(vec![2, 3, 4, 5]));
        assert_eq!(omit_of(&["-o", "8"]), Some(vec![8]));
        assert_eq!(sig_of(&["-o", "8"]), Some(9));
        assert_eq!(omit_of(&["-15", "-omit", "9"]), Some(vec![9]));
        assert_eq!(omit_of(&["-15", "o", "9"]), Some(vec![9]));
        assert_eq!(omit_of(&["-15", "-o", "8", "-o", "9"]), Some(vec![8, 9]));
        // The list is not stripped of a `-`: "-3" is no id.
        assert_eq!(omit_of(&["-15", "-o", "-3"]), Some(vec![]));
        assert_eq!(omit_of(&["-o"]), None);
        assert_eq!(omit_of(&["-15", "-o"]), None);
        // killall5 knows no %PPID.
        let (a, heard) = kill_args(&["-15", "-o", "x,,%PPID,7"]);
        assert_eq!(
            a,
            Some(KillArgs {
                sig: 15,
                omit: vec![7]
            })
        );
        assert_eq!(
            heard,
            [
                b"illegal omit pid value (x)!\n".to_vec(),
                b"illegal omit pid value ()!\n".to_vec(),
                b"illegal omit pid value (%PPID)!\n".to_vec(),
            ]
        );
        // Complaints made before the usage are made.
        let (a, heard) = kill_args(&["-o", "x", "-15"]);
        assert_eq!(a, None);
        assert_eq!(heard, [b"illegal omit pid value (x)!\n".to_vec()]);
    }

    /// A `stat` line as Linux writes one, with the fields `readproc` reads
    /// set and the rest plausible -- `tpgid` -1, as for a process with no
    /// terminal, and an `rsslim` of `RLIM_INFINITY`.
    fn stat_line(pid: &str, comm: &str, state: char, sid: i32, codes: (u64, u64)) -> String {
        let (start, end) = codes;
        format!(
            "{pid} ({comm}) {state} 1 {pid} {sid} 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 \
             12345 8000000 300 18446744073709551615 {start} {end} 140737 0 0 0 0 0 0 0 17 \
             3 0 0 0 0 0\n"
        )
    }

    /// A process directory under `dir`: its `stat` and, unless `None`, its
    /// `cmdline`.
    fn put(dir: &scratchdir::ScratchDir, name: &str, stat: &[u8], cmdline: Option<&[u8]>) {
        let d = dir.path(name);
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("stat"), stat).unwrap();
        if let Some(c) = cmdline {
            std::fs::write(d.join("cmdline"), c).unwrap();
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn proc_is_read_as_upstream_reads_it() {
        let dir = scratchdir::ScratchDir::new("killall5");
        let code = (0x40_0000, 0x50_0000);
        let line = |pid: &str, comm: &str, state: char, sid: i32, codes: (u64, u64)| {
            stat_line(pid, comm, state, sid, codes).into_bytes()
        };
        put(
            &dir,
            "1",
            &line("1", "init", 'S', 1, code),
            Some(b"/sbin/init\0splash\0"),
        );
        put(&dir, "2", &line("2", "kthreadd", 'S', 0, (0, 0)), Some(b""));
        put(
            &dir,
            "42",
            &line("42", "a) (b", 'R', 7, code),
            Some(b"bash\0-c\0-x\0script\0"),
        );
        put(&dir, "7", &line("7", "zed", 'Z', 3, (0, 0)), Some(b""));
        put(&dir, "9", b"", Some(b"x\0"));
        put(&dir, "10", b"10 (unterminated R 1 2 3\n", Some(b"x\0"));
        put(&dir, "11", b"11 (short) R 1 2\n", Some(b"x\0"));
        put(
            &dir,
            "self",
            &line("99", "self", 'S', 1, code),
            Some(b"x\0"),
        );
        put(
            &dir,
            "12abc",
            &line("12", "odd", 'S', 1, code),
            Some(b"odd\0"),
        );
        std::fs::create_dir(dir.path("13")).unwrap();
        put(&dir, "14", &line("14", "nocmd", 'S', 1, code), None);
        put(
            &dir,
            "15",
            &line("15", "anon", 'S', 1, code),
            Some(b"\0arg\0"),
        );
        let mut long = vec![b'x'; 5000];
        long.push(0);
        put(&dir, "16", &line("16", "long", 'S', 1, code), Some(&long));
        put(
            &dir,
            "17",
            &line("17", "bash", 'S', 17, code),
            Some(b"-bash\0"),
        );
        put(
            &dir,
            "18",
            &line("18", "prog", 'S', 1, code),
            Some(b"prog\0-a\0\0after\0"),
        );
        put(&dir, "20", &line("20", "a b", 'S', 1, code), Some(b"a b\0"));
        put(
            &dir,
            "21",
            b"21 noparens S 1 1 5 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 1 1 1 1 1\n",
            Some(b"np\0"),
        );
        // What follows a NUL in `stat` is not read: here, the name's `)`.
        let mut nul = line("22", "nul", 'S', 1, code);
        nul.insert(5, 0);
        put(&dir, "22", &nul, Some(b"nul\0"));

        let mut heard = Vec::new();
        let plist =
            scan_procs(dir.dir(), dir.dir(), false, &mut |m| heard.push(m.to_vec())).unwrap();
        heard.sort();
        let mut want_heard = vec![
            b"can't read from 9/stat\n".to_vec(),
            b"can't get program name from /proc/10/stat\n".to_vec(),
            b"can't read sid from 11/stat\n".to_vec(),
            b"can't get program name from /proc/22/stat\n".to_vec(),
        ];
        want_heard.sort();
        assert_eq!(heard, want_heard);

        // Highest first: the reverse of the order the directory lists them.
        let kept = [1, 2, 42, 12, 15, 16, 17, 18, 20, 21];
        let mut listed: Vec<i32> = std::fs::read_dir(dir.dir())
            .unwrap()
            .map(|e| atoi(&os_bytes(&e.unwrap().file_name())))
            .filter(|p| kept.contains(p))
            .collect();
        listed.reverse();
        assert_eq!(plist.iter().map(|p| p.pid).collect::<Vec<_>>(), listed);

        let get = |pid: i32| plist.iter().find(|p| p.pid == pid).unwrap();
        let init = get(1);
        assert_eq!(init.argv0.as_deref(), Some(&b"/sbin/init"[..]));
        assert_eq!(init.argv1.as_deref(), Some(&b"splash"[..]));
        assert_eq!(init.statname, b"init");
        assert_eq!((init.sid, init.kernel), (1, false));
        let kthread = get(2);
        assert!(kthread.kernel);
        assert_eq!(
            (kthread.argv0.as_ref(), kthread.argv1.as_ref()),
            (None, None)
        );
        // The name runs to the last `)`.
        let odd = get(42);
        assert_eq!(odd.statname, b"a) (b");
        assert_eq!(odd.sid, 7);
        assert_eq!(odd.argv0.as_deref(), Some(&b"bash"[..]));
        assert_eq!(
            odd.argv1.as_deref(),
            Some(&b"script"[..]),
            "past the options"
        );
        // `atoi` of the directory's name.
        assert_eq!(get(12).statname, b"odd");
        let anon = get(15);
        assert_eq!(anon.argv0, None);
        assert_eq!(anon.argv1.as_deref(), Some(&b"arg"[..]));
        let long = get(16);
        assert_eq!(long.argv0.as_ref().map(Vec::len), Some(4096));
        assert_eq!(long.argv1.as_ref().map(Vec::len), Some(904));
        let login = get(17);
        assert_eq!(login.argv0.as_deref(), Some(&b"-bash"[..]));
        assert_eq!(login.argv1, None);
        // An empty argument ends the search for argv[1], finding none.
        assert_eq!(get(18).argv1, None);
        assert_eq!(get(20).statname, b"a b");
        // A name without parentheses runs to the next blank.
        let np = get(21);
        assert_eq!(np.statname, b"noparens");
        assert_eq!(np.sid, 5);
        // No links were made: no executable could be read.
        assert!(plist.iter().all(|p| p.pathname.is_none()));

        // With -z, the zombie too.
        let plist = scan_procs(dir.dir(), dir.dir(), true, &mut |_| {}).unwrap();
        let zed = plist.iter().find(|p| p.pid == 7).unwrap();
        assert_eq!(zed.statname, b"zed");
        assert!(zed.kernel, "a zombie has no code addresses");

        assert_eq!(
            scan_procs(&dir.path("missing"), dir.dir(), false, &mut |_| {}),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_executable_is_read_from_its_link() {
        let dir = scratchdir::ScratchDir::new("killall5");
        let stat = stat_line("5", "app", 'S', 1, (1, 2));
        put(&dir, "5", stat.as_bytes(), Some(b"app\0"));
        std::os::unix::fs::symlink("/usr/bin/app (deleted)", dir.path("5").join("exe")).unwrap();
        let plist = scan_procs(dir.dir(), dir.dir(), false, &mut |_| {}).unwrap();
        assert_eq!(
            plist[0].pathname.as_deref(),
            Some(&b"/usr/bin/app (deleted)"[..])
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_real_proc_holds_this_process() {
        let plist = scan_procs(Path::new("/proc"), Path::new("/proc"), false, &mut |_| {}).unwrap();
        let me = i32::try_from(std::process::id()).unwrap();
        let p = plist.iter().find(|p| p.pid == me).expect("this process");
        let comm = std::fs::read("/proc/self/comm").unwrap();
        assert_eq!(p.statname, comm.strip_suffix(b"\n").unwrap());
        let argv0 = std::env::args_os().next().unwrap();
        assert_eq!(p.argv0.as_deref(), Some(os_bytes(&argv0).as_ref()));
        assert_eq!(Some(p.sid), libcall::process::session_of(0).ok());
        assert!(!p.kernel);
        let exe = std::fs::read_link("/proc/self/exe").unwrap();
        assert_eq!(
            p.pathname.as_deref(),
            Some(os_bytes(exe.as_os_str()).as_ref())
        );
    }

    /// Arguments that look for `names`, with nothing else asked.
    fn looking_for(names: &[&str]) -> PidofArgs {
        PidofArgs {
            omit: Vec::new(),
            single: false,
            quiet: false,
            chroot_check: false,
            sep: b' ',
            scripts_too: 0,
            list_dz: false,
            names: names.iter().map(|n| n.as_bytes().to_vec()).collect(),
        }
    }

    /// What `report` prints, and whether it found anything.
    fn printed(plist: &[Proc], args: &PidofArgs) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        let any = report(plist, args, true, None, &mut out);
        (out, any)
    }

    #[test]
    fn pidof_prints_each_process_found_with_its_separator() {
        let plist = [
            proc(30, "alpha", "", "alpha", ""),
            proc(20, "alpha", "", "alpha", ""),
            proc(10, "beta", "", "beta", ""),
        ];
        let mut a = looking_for(&["alpha", "beta"]);
        assert_eq!(printed(&plist, &a), (b"30 20 10\n".to_vec(), true));
        a.sep = b',';
        assert_eq!(printed(&plist, &a).0, b"30,20,10\n");
        a.sep = 0;
        assert_eq!(printed(&plist, &a).0, b"30\x0020\x0010\n");
        a.sep = b' ';
        // -s: one for each name.
        a.single = true;
        assert_eq!(printed(&plist, &a).0, b"30 10\n");
        // One left out does not use up -s.
        a.omit = vec![30];
        assert_eq!(printed(&plist, &a).0, b"20 10\n");
        a.single = false;
        assert_eq!(printed(&plist, &a).0, b"20 10\n");
        // -q: nothing printed, and still found.
        a.quiet = true;
        assert_eq!(printed(&plist, &a), (Vec::new(), true));
    }

    #[test]
    fn pidof_prints_a_name_given_twice_twice_and_nothing_for_none() {
        let plist = [proc(10, "beta", "", "beta", "")];
        assert_eq!(
            printed(&plist, &looking_for(&["beta", "beta"])),
            (b"10 10\n".to_vec(), true)
        );
        assert_eq!(
            printed(&plist, &looking_for(&["gamma"])),
            (Vec::new(), false)
        );
        assert_eq!(printed(&plist, &looking_for(&[])), (Vec::new(), false));
        assert_eq!(
            printed(&plist, &looking_for(&["beta/", "beta"])),
            (b"10\n".to_vec(), true)
        );
    }
}
