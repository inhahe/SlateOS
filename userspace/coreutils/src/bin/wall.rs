//! `wall` -- write a message to all users: util-linux 2.39.3's, ported, with
//! Ubuntu's fix for CVE-2024-28085.
//!
//! ```text
//! wall [options] [<file> | <message>]
//! ```
//!
//! A transcription of `term-utils/wall.c` and the `ttymsg.c` it writes
//! through, as Ubuntu 24.04 builds them. The message is built once, then
//! written to the terminal of every user process in utmp. What upstream does
//! and this keeps:
//!
//! - **The message** (`makemsg`): a blank line, the banner -- `Broadcast
//!   message from USER@HOST (TTY) (DATE):`, cut and padded to 79 columns and
//!   followed by two bells -- unless `-n`, which only root may give; then the
//!   message, each line ended `\r\n`; then a blank line. The text is the
//!   words on the command line, or the lines of the one operand when it
//!   names an existing file, or of standard input -- every piece of it
//!   through `fputs_careful`, controls shown as `^X`, wrapped at 79 columns.
//!   The command-line words go through it too: that is Ubuntu's patch for
//!   CVE-2024-28085, where 2.39.3 wrote them as they were and so let anyone
//!   put escape sequences on every terminal. Each word is its own call, so
//!   the wrapping column starts again at each.
//! - **Who receives it**: each utmp entry with a user, of type
//!   `USER_PROCESS`, whose line is neither empty nor an X display (`:0`) --
//!   and with `-g GROUP`, whose user is in the group, by `getgrouplist`, as
//!   upstream asks it: with the index one past the list's end compared too,
//!   so a group whose gid is 0 takes in everybody.
//! - **The writing** (`ttymsg`): `/dev/LINE` opened `O_NONBLOCK`; a line
//!   that is busy, forbidden or gone skipped in silence; a write that would
//!   block handed to a child, which -- upstream's `fcntl` clearing
//!   `O_NONBLOCK` on the wrong descriptor -- tries once more and gives up;
//!   and a failed write closed through util-linux's `close_fd`, whose
//!   `fsync` fails on any terminal, so what is reported is `Invalid
//!   argument` rather than the write's own error, as upstream reports it.
//! - **`-t SECONDS`**, the child's alarm, read by `strtou32_or_err`.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - Names and arguments echoed in a diagnostic have their unprintable bytes
//!   escaped (`quoting::escape_unprintable`), so none can forge a line of its
//!   own; upstream prints the bytes. The message itself is upstream's.

use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes, os_from_bytes};
use coreutils::stdfd;
use coreutils::stdio::StdioReader;

coreutils::guard_std_fds!();

/// The parser. glibc's getopt names the program by `argv[0]` in its own
/// complaints, util-linux by `argv[0]`'s last component.
const WALL: Program = Program::new("wall", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "nt:g:Vh";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("nobanner", Takes::Nothing),
    ("timeout", Takes::Required),
    ("group", Takes::Required),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];

/// `TERM_WIDTH`: "we wrap message lines at column 79, not 80, because some
/// terminals wrap after 79, some do not, and we can't tell."
const TERM_WIDTH: usize = 79;

/// `WRITE_TIME_OUT`, in seconds.
const WRITE_TIME_OUT: u32 = 300;

/// `_PATH_UTMPX`, which `getutxent` reads.
const PATH_UTMPX: &str = "/var/run/utmp";

/// `sysconf (_SC_NGROUPS_MAX)` on Linux, and SlateOS's.
const NGROUPS_MAX: usize = 65_536;

/// `MAXNAMLEN`, the size of `ttymsg`'s device buffer.
const MAXNAMLEN: usize = 255;

/// `sizeof (lbuf)` for the banner: `snprintf` cuts it there.
const LBUF_LEN: usize = 512;

/// `errno` values `ttymsg` acts on.
const EACCES: i32 = 13;
const EBUSY: i32 = 16;
const ENOENT: i32 = 2;
const EAGAIN: i32 = 11;
const ENODEV: i32 = 19;
const EIO: i32 = 5;
const SIGTERM: i32 = 15;

/// A run that has ended with this status, its diagnostic already out.
#[derive(Debug)]
struct Die(u8);

/// Bytes from the user or the file system, made safe to print in a
/// diagnostic.
fn shown(s: &[u8]) -> Vec<u8> {
    escape_unprintable(s).into_bytes()
}

/// util-linux's `warn`/`warnx`: `PROG: PARTS[: REASON]` on standard error,
/// standard output not flushed first.
fn warn(prog: &[u8], parts: &[&[u8]], reason: Option<&std::io::Error>) {
    let mut m = prog.to_vec();
    m.extend_from_slice(b": ");
    for p in parts {
        m.extend_from_slice(p);
    }
    if let Some(e) = reason {
        m.extend_from_slice(b": ");
        m.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
    }
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
}

/// `errtryhelp (EXIT_FAILURE)`'s referral.
fn try_help(prog: &[u8]) {
    let mut m = b"Try '".to_vec();
    m.extend_from_slice(prog);
    m.extend_from_slice(b" --help' for more information.\n");
    ulclosestream::stderr_write(&m);
}

/// `usage`, to standard output.
fn usage(prog: &[u8]) -> Vec<u8> {
    let mut s = b"\nUsage:\n ".to_vec();
    s.extend_from_slice(prog);
    s.extend_from_slice(b" [options] [<file> | <message>]\n");
    s.extend_from_slice(b"\nWrite a message to all users.\n");
    s.extend_from_slice(b"\nOptions:\n");
    s.extend_from_slice(b" -g, --group <group>     only send message to group\n");
    s.extend_from_slice(b" -n, --nobanner          do not print banner, works only for root\n");
    s.extend_from_slice(b" -t, --timeout <timeout> write timeout in seconds\n");
    s.push(b'\n');
    s.extend_from_slice(format!("{:<25}{}\n", " -h, --help", "display this help").as_bytes());
    s.extend_from_slice(format!("{:<25}{}\n", " -V, --version", "display version").as_bytes());
    s.extend_from_slice(b"\nFor more details see wall(1).\n");
    s
}

/// `strtou32_or_err (arg, errmesg)`.
fn strtou32_or_err(prog: &[u8], arg: &[u8], errmesg: &str) -> Result<u32, Die> {
    let parsed = ulstrutils::ul_strtou64(arg, 10)
        .and_then(|n| u32::try_from(n).map_err(|_| ulstrutils::NumErr::Range));
    parsed.map_err(|e| {
        let msg = ulstrutils::num_error_message(errmesg, &os_from_bytes(arg), e);
        warn(prog, &[msg.as_bytes()], None);
        Die(1)
    })
}

/// `struct group_workspace`: the group asked for, and the buffer
/// `getgrouplist` fills -- kept between calls, as upstream keeps it.
struct GroupWorkspace {
    requested_group: u32,
    groups: Vec<u32>,
}

impl GroupWorkspace {
    /// `init_group_workspace`, through `get_group_gid`.
    fn new(prog: &[u8], db: &pwdb::Db, group: &[u8]) -> Result<Self, Die> {
        let requested_group = match db.group_by_name(group) {
            Some(g) => g.gid,
            None => {
                let gid = strtou32_or_err(prog, group, "invalid group argument")?;
                if db.group_by_gid(gid).is_none() {
                    warn(prog, &[&shown(group), b": unknown gid"], None);
                    return Err(Die(1));
                }
                gid
            }
        };
        Ok(GroupWorkspace {
            requested_group,
            // `xcalloc`: zeros, room for the primary gid as well.
            groups: vec![0; NGROUPS_MAX.saturating_add(1)],
        })
    }

    /// `is_gr_member`. Upstream's loop starts at `groups[ngroups]`, one past
    /// what `getgrouplist` wrote, so whatever an earlier call left there --
    /// a zero, from the allocation, until a longer list overwrites it -- is
    /// compared as though it were one of this user's groups.
    fn is_member(&mut self, db: &pwdb::Db, login: &[u8]) -> bool {
        let Some(pw) = db.user_by_name(login) else {
            return false;
        };
        if self.requested_group == pw.gid {
            return true;
        }
        let list = db.group_list(login, pw.gid);
        let n = list.len();
        if n > self.groups.len() {
            // "getgrouplist found more groups than sysconf allows": upstream
            // exits; the list cannot outgrow the buffer here, which holds
            // every group the database has room for.
            return false;
        }
        if let Some(head) = self.groups.get_mut(..n) {
            head.copy_from_slice(&list);
        }
        (0..=n)
            .rev()
            .any(|k| self.groups.get(k) == Some(&self.requested_group))
    }
}

/// `xgethostname`: `gethostname` into `sysconf (_SC_HOST_NAME_MAX) + 1`
/// bytes, or `None`.
fn xgethostname() -> Option<Vec<u8>> {
    let mut buf = [0u8; 65];
    let n = libcall::hostname_into(&mut buf).ok()?;
    Some(buf.get(..n.min(64)).unwrap_or_default().to_vec())
}

/// util-linux's `xgetlogin`: the real uid's name, or `None`.
fn xgetlogin(db: &pwdb::Db) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        let uid = coreutils::grouplist::current_ids().ruid;
        db.user_by_uid(uid)
            .map(|u| u.name.clone())
            .filter(|n| !n.is_empty())
    }
    #[cfg(not(unix))]
    {
        let _ = db;
        None
    }
}

/// `geteuid ()`, `getuid ()`, `getgid ()` and `getegid ()`.
fn ids() -> (u32, u32, u32, u32) {
    #[cfg(unix)]
    {
        let i = coreutils::grouplist::current_ids();
        (i.euid, i.ruid, i.rgid, i.egid)
    }
    #[cfg(not(unix))]
    {
        (0, 0, 0, 0)
    }
}

/// `ttyname (fd)`, or `None` for a descriptor that is no terminal.
fn ttyname(fd: i32) -> Option<Vec<u8>> {
    let mut buf = [0u8; 4096];
    let n = libcall::termios::ttyname_into(fd, &mut buf).ok()?;
    buf.get(..n).map(<[u8]>::to_vec)
}

/// `%*s` of `" "` in `TERM_WIDTH`, then `\r\n`: a blank line.
fn blank_line(fs: &mut Vec<u8>) {
    fs.resize(fs.len().saturating_add(TERM_WIDTH), b' ');
    fs.extend_from_slice(b"\r\n");
}

/// `makemsg`: the message every terminal is sent.
fn makemsg(
    prog: &[u8],
    db: &pwdb::Db,
    fname: Option<&[u8]>,
    mvec: &[Vec<u8>],
    print_banner: bool,
) -> Result<Vec<u8>, Die> {
    let mut fs = Vec::new();
    if print_banner {
        let hostname = xgethostname();
        let whom = match xgetlogin(db) {
            Some(w) => w,
            None => {
                // `errno` is 0 when the uid has no entry: glibc's `getpwuid`
                // finding nothing is no error.
                warn(
                    prog,
                    &[b"cannot get passwd uid"],
                    Some(&std::io::Error::from_raw_os_error(0)),
                );
                b"<someone>".to_vec()
            }
        };
        let where_ = match ttyname(1) {
            None => b"somewhere".to_vec(),
            Some(t) => t
                .strip_prefix(b"/dev/")
                .map_or_else(|| t.clone(), <[u8]>::to_vec),
        };
        let zone = localtime::Zone::from_env();
        let date = localtime::asctime(&zone.localtime(wall_clock(), 0)).unwrap_or_default();

        // "all this stuff is to blank out a square for the message"
        fs.push(b'\r');
        blank_line(&mut fs);

        let mut lbuf = b"Broadcast message from ".to_vec();
        lbuf.extend_from_slice(&whom);
        lbuf.push(b'@');
        lbuf.extend_from_slice(hostname.as_deref().unwrap_or(b"(null)"));
        lbuf.extend_from_slice(b" (");
        lbuf.extend_from_slice(&where_);
        lbuf.extend_from_slice(b") (");
        lbuf.extend_from_slice(&date);
        lbuf.extend_from_slice(b"):");
        lbuf.truncate(LBUF_LEN.saturating_sub(1));
        // `"%-*.*s\007\007\r\n"`, 79 and 79.
        let shown_part = lbuf.get(..lbuf.len().min(TERM_WIDTH)).unwrap_or_default();
        fs.extend_from_slice(shown_part);
        fs.resize(
            fs.len()
                .saturating_add(TERM_WIDTH.saturating_sub(shown_part.len())),
            b' ',
        );
        fs.extend_from_slice(b"\x07\x07\r\n");
    }
    blank_line(&mut fs);

    if mvec.is_empty() {
        let mut input = match fname {
            Some(name) => {
                // "When we are not root, but suid or sgid, refuse to read
                // files ... After all, our invoker can easily do "wall <
                // file" instead of "wall file"."
                let (euid, uid, gid, egid) = ids();
                if uid != 0 && (uid != euid || gid != egid) {
                    warn(
                        prog,
                        &[b"will not read ", &shown(name), b" - use stdin."],
                        None,
                    );
                    return Err(Die(1));
                }
                // `freopen (fname, "r", stdin)`.
                let file = std::fs::File::open(os_from_bytes(name)).map_err(|e| {
                    warn(prog, &[b"cannot open ", &shown(name)], Some(&e));
                    Die(1)
                })?;
                StdioReader::from_file(file)
            }
            None => StdioReader::stdin(),
        };
        // `while (getline (&lbuf, &lbuflen, stdin) >= 0)`: to the end of the
        // input, or its first error, which upstream does not report.
        loop {
            let mut line = Vec::new();
            match input.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => fs.extend(ulstrutils::fputs_careful(&line, b'^', true, TERM_WIDTH)),
            }
        }
    } else {
        // Ubuntu's fix for CVE-2024-28085: each word through fputs_careful,
        // as every other path is.
        for (i, word) in mvec.iter().enumerate() {
            fs.extend(ulstrutils::fputs_careful(word, b'^', true, TERM_WIDTH));
            if i.saturating_add(1) < mvec.len() {
                fs.push(b' ');
            }
        }
        fs.extend_from_slice(b"\r\n");
    }
    blank_line(&mut fs);
    Ok(fs)
}

/// `time (NULL)`.
fn wall_clock() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_secs())
            .unwrap_or(i64::MAX)
            .saturating_neg(),
    }
}

/// `errno` of an I/O error, 0 when it carries none.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

/// `"%s: %m"`, the device's unprintable bytes escaped.
fn device_error(device: &[u8], errno: i32) -> Vec<u8> {
    let mut m = shown(device);
    m.extend_from_slice(b": ");
    m.extend_from_slice(
        coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
    );
    m
}

/// util-linux's `close_fd`: `fsync` then `close`, and `errno` as the last
/// of them to fail left it -- `None` when both succeeded.
fn close_fd(file: std::fs::File) -> Option<i32> {
    let mut errno = None;
    if let Err(e) = file.sync_all() {
        errno = Some(errno_of(&e));
    }
    if let Err(e) = stdfd::close(file) {
        errno = Some(errno_of(&e));
    }
    errno
}

/// `ttymsg (&iov, 1, line, tmout)`: the message onto `/dev/LINE`. `None` when
/// it went, or when the line is one upstream skips in silence; else the
/// complaint, which the caller prints.
fn ttymsg(prog: &[u8], msg: &[u8], line: &[u8], tmout: u32) -> Option<Vec<u8>> {
    let mut device = b"/dev/".to_vec();
    device.extend_from_slice(line);
    if device.len() >= MAXNAMLEN {
        return Some(b"excessively long line arg".to_vec());
    }

    // "open will fail on slip lines or exclusive-use lines if not running as
    // root; not an error."
    let mut file = match open_nonblocking(&device) {
        Ok(f) => f,
        Err(e) => {
            let errno = errno_of(&e);
            if errno == EBUSY || errno == EACCES || errno == ENOENT {
                return None;
            }
            return Some(device_error(&device, errno));
        }
    };

    let mut left = msg;
    let mut forked = false;
    loop {
        match file.write(left) {
            Ok(n) if n >= left.len() => break,
            Ok(n) => left = left.get(n..).unwrap_or_default(),
            Err(e) if errno_of(&e) == EAGAIN => {
                if forked {
                    drop(stdfd::close(file));
                    exit_now(1);
                }
                // SAFETY: wall is single-threaded, so the child may do what
                // it likes; and it does little -- write, alarm, close and
                // `_exit`, never returning to `wall`'s own loop.
                match unsafe { libcall::process::fork() } {
                    Err(errno) => {
                        drop(stdfd::close(file));
                        let mut m = b"fork: ".to_vec();
                        m.extend_from_slice(
                            coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
                                .as_bytes(),
                        );
                        return Some(m);
                    }
                    Ok(libcall::process::Forked::Parent(_)) => {
                        drop(stdfd::close(file));
                        return None;
                    }
                    Ok(libcall::process::Forked::Child) => {
                        forked = true;
                        // "wait at most tmout seconds"
                        let _ = libcall::signal::set_default(libcall::signal::SIGALRM);
                        let _ = libcall::signal::set_default(SIGTERM);
                        let _ = libcall::signal::set_mask(&libcall::signal::SigSet::empty());
                        let _ = libcall::signal::alarm(tmout);
                        // Upstream's own call, `fcntl (flags, F_SETFL, ...)`:
                        // the flags are read from the terminal and set on the
                        // descriptor whose number they happen to be, so the
                        // terminal stays non-blocking, the next write would
                        // block again, and the child gives up there.
                        let fd = raw_fd(&file);
                        if let Ok(flags) = libcall::termios::status_flags(fd) {
                            let _ = libcall::termios::set_status_flags(
                                flags,
                                flags & !libcall::termios::O_NONBLOCK,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                let errno = errno_of(&e);
                // "We get ENODEV on a slip line if we're running as root,
                // and EIO if the line just went away."
                if errno == ENODEV || errno == EIO {
                    break;
                }
                let errno = match close_fd(file) {
                    Some(close_errno) => {
                        warn(
                            prog,
                            &[b"write failed: ", &shown(&device)],
                            Some(&std::io::Error::from_raw_os_error(close_errno)),
                        );
                        close_errno
                    }
                    None => errno,
                };
                if forked {
                    exit_now(1);
                }
                return Some(device_error(&device, errno));
            }
        }
    }
    drop(stdfd::close(file));
    if forked {
        exit_now(0);
    }
    None
}

/// `_exit (status)`.
fn exit_now(status: i32) -> ! {
    #[cfg(unix)]
    {
        libcall::process::exit_immediately(status)
    }
    #[cfg(not(unix))]
    {
        std::process::exit(status)
    }
}

/// The descriptor under `file`.
#[cfg(unix)]
fn raw_fd(file: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

#[cfg(not(unix))]
fn raw_fd(_file: &std::fs::File) -> i32 {
    -1
}

/// `open (device, O_WRONLY | O_NONBLOCK)`.
fn open_nonblocking(device: &[u8]) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libcall::termios::O_NONBLOCK);
    }
    opts.open(os_from_bytes(device))
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    // `program_invocation_short_name`: what follows the last slash.
    let short = argv0
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(argv0.as_slice(), |i| {
            argv0.get(i.saturating_add(1)..).unwrap_or_default()
        })
        .to_vec();
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(argv.get(1..).unwrap_or_default(), &argv0, &short, &mut out) {
        Ok(code) | Err(Die(code)) => code,
    };
    ExitCode::from(out.close(status, &short))
}

/// Upstream's `main`, after `setlocale`.
fn run(
    argv: &[OsString],
    argv0: &[u8],
    short: &[u8],
    out: &mut ulclosestream::Stdout,
) -> Result<u8, Die> {
    let prog = shown(short);
    let db = pwdb::Db::load();
    let mut print_banner = true;
    let mut group: Option<GroupWorkspace> = None;
    let mut timeout = WRITE_TIME_OUT;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    for item in WALL.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                let mut m = shown(argv0);
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                ulclosestream::stderr_write(&m);
                try_help(&prog);
                return Err(Die(1));
            }
        };
        let arg = |v: Option<OsString>| v.as_deref().map(os_bytes).unwrap_or_default().into_owned();
        match opt {
            Opt::Short(b'n', _) | Opt::Long("nobanner", _) => {
                if ids().0 == 0 {
                    print_banner = false;
                } else {
                    warn(&prog, &[b"--nobanner is available only for root"], None);
                }
            }
            Opt::Short(b't', v) | Opt::Long("timeout", v) => {
                let text = arg(v);
                timeout = strtou32_or_err(&prog, &text, "invalid timeout argument")?;
                if timeout < 1 {
                    warn(&prog, &[b"invalid timeout argument: ", &shown(&text)], None);
                    return Err(Die(1));
                }
            }
            Opt::Short(b'g', v) | Opt::Long("group", v) => {
                group = Some(GroupWorkspace::new(&prog, &db, &arg(v))?);
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                let mut v = prog.clone();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                out.write(&v);
                return Ok(0);
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                out.write(&usage(&prog));
                return Ok(0);
            }
            Opt::Operand(v) => operands.push(os_bytes(v).into_owned()),
            Opt::Short(..) | Opt::Long(..) => {
                try_help(&prog);
                return Err(Die(1));
            }
        }
    }

    // One operand that names something that exists is a file; anything else
    // is the message.
    let (fname, mvec): (Option<&[u8]>, &[Vec<u8>]) = match operands.as_slice() {
        [one] if std::fs::metadata(os_from_bytes(one)).is_ok() => (Some(one.as_slice()), &[]),
        rest => (None, rest),
    };

    let mbuf = makemsg(&prog, &db, fname, mvec, print_banner)?;

    // `getutxent` over `_PATH_UTMPX`: a file that is not there is no users.
    let records = std::fs::read(PATH_UTMPX)
        .map(|data| utmpfile::parse(&data))
        .unwrap_or_default();
    for u in &records {
        if u.user.first().is_none_or(|&c| c == 0) {
            continue;
        }
        if u.record_type != utmpfile::USER_PROCESS {
            continue;
        }
        // "use-sessreg in /etc/X11/wdm/ produces ut_line entries like :0,
        // and a write to /dev/:0 fails. It also seems that some login
        // manager may produce empty ut_line."
        if u.tty.first().is_none_or(|&c| c == b':') {
            continue;
        }
        if let Some(g) = group.as_mut()
            && !g.is_member(&db, &u.user)
        {
            continue;
        }
        if let Some(p) = ttymsg(&prog, &mbuf, &u.tty, timeout) {
            warn(&prog, &[&p], None);
        }
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn db() -> pwdb::Db {
        pwdb::Db::from_bytes(
            b"root:x:0:0::/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/sh\nbob:x:1001:100::/home/bob:/bin/sh\nerin:x:1002:100::/home/erin:/bin/sh\n",
            b"root:x:0:\nusers:x:100:\nalice:x:1000:\nwheel:x:10:alice\nstaff:x:50:bob,alice\n",
        )
    }

    #[test]
    fn the_message_is_framed_and_its_words_escaped() {
        let m = makemsg(
            b"wall",
            &db(),
            None,
            &[b"hi\x1b[2J".to_vec(), b"there".to_vec()],
            false,
        )
        .unwrap();
        let blank = [b" ".repeat(TERM_WIDTH), b"\r\n".to_vec()].concat();
        let want = [blank.clone(), b"hi^[[2J there\r\n".to_vec(), blank].concat();
        assert_eq!(m, want);
    }

    #[test]
    fn group_membership_is_upstreams_off_the_end_look_included() {
        let db = db();
        let mut wheel = GroupWorkspace {
            requested_group: 10,
            groups: vec![0; 8],
        };
        assert!(wheel.is_member(&db, b"alice"));
        assert!(!wheel.is_member(&db, b"bob"));
        assert!(!wheel.is_member(&db, b"nosuch"));
        // The primary group is enough.
        let mut users = GroupWorkspace {
            requested_group: 100,
            groups: vec![0; 8],
        };
        assert!(users.is_member(&db, b"bob"));
        // gid 0: the zero one past bob's list matches -- upstream's
        // off-by-one, which takes in everybody.
        let mut root = GroupWorkspace {
            requested_group: 0,
            groups: vec![0; 8],
        };
        assert!(root.is_member(&db, b"bob"));
        // And a stale entry past a shorter list counts: alice's list,
        // [1000, 10, 50], leaves wheel's 10 at index 1, where erin's list --
        // [100], one long -- is not, and she is let in.
        let mut wheel = GroupWorkspace {
            requested_group: 10,
            groups: vec![0; 8],
        };
        assert!(!wheel.is_member(&db, b"erin"));
        assert!(wheel.is_member(&db, b"alice"));
        assert!(wheel.is_member(&db, b"erin"));
    }

    #[test]
    fn timeouts_are_read_as_strtou32_reads_them() {
        assert_eq!(strtou32_or_err(b"wall", b"30", "x").ok(), Some(30));
        assert!(strtou32_or_err(b"wall", b"4294967296", "x").is_err());
        assert!(strtou32_or_err(b"wall", b"-1", "x").is_err());
        assert!(strtou32_or_err(b"wall", b"", "x").is_err());
    }
}
