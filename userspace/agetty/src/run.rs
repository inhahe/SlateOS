//! The run on a line -- `main` after the command line: open the line as
//! standard input, output and error (`open_tty`), set its modes
//! (`termio_init`), show the issue and the prompt, read the name
//! (`get_logname`), set the modes the name showed (`termio_final`), and run
//! `login` with it.
//!
//! Every system call is upstream's, in upstream's order, with its
//! diagnostics; what decides anything is in the modules this calls --
//! [`crate::termio`], [`crate::logname`], [`crate::login`],
//! [`crate::prompt`].

use std::ffi::CString;
use std::fs::File;
use std::mem::ManuallyDrop;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::FromRawFd;

use libcall::termios::{self as tc, Termios};

use crate::issue::{Eval, Issue, Paths, SystemHost, issue_is_changed};
use crate::logindefs::LoginDefs;
use crate::logname::{Port, Read, finish_name, read_name};
use crate::options::{
    Clocal, F_EIGHTBITS, F_HANGUP, F_INITSTRING, F_KEEPCFLAGS, F_KEEPSPEED, F_LOGINPAUSE,
    F_NOCLEAR, F_NOHINTS, F_NOHOSTNAME, F_NONL, F_NOPROMPT, F_PARSE, F_UTF8, F_VCONSOLE,
    F_WAITCRLF, Options,
};
use crate::termio::{self, Chardata};
use crate::{Diag, Exit, RELOAD_FILE, System, strerror};

/// `DEFAULT_VCTERM`: a virtual console's terminal type.
const DEFAULT_VCTERM: &[u8] = b"linux";
/// `DEFAULT_STERM`: a serial line's.
const DEFAULT_STERM: &[u8] = b"vt102";
/// `_PATH_TTYS`.
const PATH_TTYS: &str = "/etc/ttys";
/// `_PATH_LOGINDEFS`.
const PATH_LOGINDEFS: &str = "/etc/login.defs";
/// `PATH_MAX`.
const PATH_MAX: usize = 4096;

/// The parts, one after another.
fn join(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `a`, then `b`.
fn cat(a: &[u8], b: &[u8]) -> Vec<u8> {
    join(&[a, b])
}

/// `/dev/TTY: what`.
fn on_dev(tty: &[u8], what: &str) -> Vec<u8> {
    join(&[b"/dev/", tty, what.as_bytes()])
}

/// A call whose result upstream does not look at: its failure changes
/// nothing that follows, and the run goes on as upstream's would. Every
/// result this module drops goes through here, so that each drop says so.
fn unchecked<T, E>(_result: Result<T, E>) {}

/// `write_all (STDOUT_FILENO, ...)`: a failure is not looked at, as
/// upstream does not look.
fn out(bytes: &[u8]) {
    unchecked(libcall::fd::write_all(1, bytes, true));
}

/// The descriptor `fd`, borrowed as a file for the calls only `std` makes,
/// and not closed after.
fn with_file<R>(fd: i32, f: impl FnOnce(&File) -> R) -> R {
    // SAFETY: `fd` is an open descriptor of this process, and the
    // `ManuallyDrop` keeps the borrowed `File` from closing it.
    let file = ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
    f(&file)
}

/// The line, standard input and output, as `get_logname` reads from it --
/// and what tells it to show the issue again. `log` is where a warning
/// about the reload file goes: the program's own diagnostics, borrowed
/// apart from the reading's.
struct Line<'a> {
    reload: &'a mut Reload,
    log: System,
}

impl Port for Line<'_> {
    fn wait_for_input(&mut self) -> bool {
        self.reload.wait_for_term_input(0, &mut self.log)
    }

    fn read_byte(&mut self) -> Result<Option<u8>, i32> {
        let mut c = [0u8; 1];
        match libcall::fd::read(0, &mut c)? {
            0 => Ok(None),
            _ => Ok(c.first().copied()),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        out(bytes);
    }

    fn pause(&mut self) {
        unchecked(libcall::clock::nanosleep(0, 250_000_000));
    }
}

/// The run's state that outlives a call: the next speed to try.
struct Speeds {
    /// `next_speed`'s `baud_index`, -1 before the first.
    index: Option<usize>,
}

/// The actions `SIGHUP`, `SIGQUIT` and `SIGINT` had before [`ignore_signals`]
/// ignored them, which a run puts back. An action that could not be read
/// and replaced is one there is nothing to put back: `None`.
pub struct Ignored {
    hup: Option<libcall::signal::SavedAction>,
    quit: Option<libcall::signal::SavedAction>,
    int: Option<libcall::signal::SavedAction>,
}

/// Ignore `SIGHUP`, `SIGQUIT` and `SIGINT` -- "in case vhangup(2) has to
/// called" -- as upstream's `main` does first, before it parses its
/// arguments: `--show-issue`, `--reload` and an option error's slow exit
/// are not interrupted either.
#[must_use]
pub fn ignore_signals() -> Ignored {
    Ignored {
        hup: libcall::signal::ignore(libcall::pty::SIGHUP).ok(),
        quit: libcall::signal::ignore(libcall::signal::SIGQUIT).ok(),
        int: libcall::signal::ignore(libcall::signal::SIGINT).ok(),
    }
}

/// Run on the line `op` names, as `main` does after `parse_args`, with the
/// signals' actions [`ignore_signals`] saved: returns only when the run
/// ends without `login` -- the status to exit with.
pub fn run(mut op: Options, sys: &mut System, ignored: Ignored) -> Exit {
    match run_inner(&mut op, sys, &ignored) {
        Ok(never) => match never {},
        Err(exit) => exit,
    }
}

/// [`run`], its every way out an `Err`.
fn run_inner(
    op: &mut Options,
    sys: &mut System,
    ignored: &Ignored,
) -> Result<std::convert::Infallible, Exit> {
    let (sa_hup, sa_quit, sa_int) = (&ignored.hup, &ignored.quit, &ignored.int);

    crate::utmp::update_utmp(op);

    if op.delay != 0 {
        unchecked(libcall::clock::nanosleep(u64::from(op.delay), 0));
    }

    let mut tp = open_tty(op, sys)?;

    // "Unmask SIGHUP if inherited".
    let mut hup = libcall::signal::SigSet::empty();
    if hup.add(libcall::pty::SIGHUP).is_ok() {
        unchecked(libcall::signal::unblock(&hup));
    }
    if let Some(saved) = &sa_hup {
        unchecked(libcall::signal::restore_action(libcall::pty::SIGHUP, saved));
    }
    let pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    unchecked(libcall::tty::set_foreground_group(0, pid));

    // "Default is to follow the current line speed and then default to
    // 9600".
    if op.flags & F_VCONSOLE == 0 && op.speeds.is_empty() {
        op.speeds.push(libcall::termios::B9600);
        op.flags |= F_KEEPSPEED;
    }

    termio_init(op, &mut tp, sys);

    if op.flags & F_INITSTRING != 0
        && let Some(init) = &op.initstring
    {
        // `strlen`: the string ends at a NUL `\000` put in it.
        let init = init.split(|&b| b == 0).next().unwrap_or_default();
        if !init.is_empty() {
            out(init);
        }
    }

    if op.flags & F_VCONSOLE != 0 || op.clocal != Clocal::Always {
        unchecked(tc::clear_nonblocking(1));
    }

    if termio::serial_option(op, F_PARSE) {
        auto_baud(&mut tp);
    }

    if op.timeout != 0 {
        // What answers is the time left on an earlier alarm: there is none.
        let _ = libcall::signal::alarm(op.timeout);
    }

    if termio::serial_option(op, F_WAITCRLF) {
        let mut ch = [0u8; 1];
        while libcall::fd::read(0, &mut ch) == Ok(1) {
            let c = ch[0] & 0x7f;
            if c == b'\n' || c == b'\r' {
                break;
            }
        }
    }

    let mut cp = Chardata::default();
    let mut issue = Issue::default();
    let mut reload = Reload {
        watch: Watch::Untried,
        netlink: None,
    };
    let mut username = op.autolog.clone();

    if op.flags & F_NOPROMPT != 0 {
        eval_issue(&mut issue, op, &tp, &mut reload, sys);
        print_issue(&mut issue, op, &mut tp);
    } else {
        if op.flags & F_NOHOSTNAME == 0
            && LoginDefs::load(std::path::Path::new(PATH_LOGINDEFS))
                .get_bool(b"LOGIN_PLAIN_PROMPT", false)
        {
            op.flags |= F_NOHOSTNAME;
        }
        if let Some(autolog) = &op.autolog {
            eval_issue(&mut issue, op, &tp, &mut reload, sys);
            do_prompt(&mut issue, op, &mut tp, &mut reload, sys);
            out(&join(&[
                crate::prompt::LOGIN_PROMPT,
                autolog,
                b" (automatic login)\n",
            ]));
        } else {
            let mut speeds = Speeds { index: None };
            loop {
                if let Some(name) = get_logname(&mut issue, op, &mut tp, &mut cp, &mut reload, sys)?
                {
                    username = Some(name);
                    break;
                }
                if op.flags & F_VCONSOLE == 0 && !op.speeds.is_empty() {
                    next_speed(op, &mut tp, &mut speeds);
                }
            }
        }
    }

    if op.timeout != 0 {
        // The time that was left, which nothing needs.
        let _ = libcall::signal::alarm(0);
    }

    if op.flags & F_VCONSOLE == 0 {
        termio_final(op, &mut tp, &cp, sys)?;
    } else {
        reset_vc(op, &mut tp, true, sys);
    }

    // "Now the newline character should be properly written."
    out(b"\r\n");

    if let Some(saved) = &sa_quit {
        unchecked(libcall::signal::restore_action(
            libcall::signal::SIGQUIT,
            saved,
        ));
    }
    if let Some(saved) = &sa_int {
        unchecked(libcall::signal::restore_action(
            libcall::signal::SIGINT,
            saved,
        ));
    }

    if let Some(name) = &username {
        crate::login::check_username(name, sys)?;
    }

    let argv = crate::login::login_argv(op, username.as_deref());

    if let Some(root) = &op.chroot {
        let path = CString::new(root.clone()).map_err(|_| libcall::ENOENT);
        if let Err(e) = path.and_then(|p| libcall::process::change_root(&p)) {
            let msg = join(&[
                &op.tty,
                b": can't change root directory ",
                root,
                b": ",
                strerror(e).as_bytes(),
            ]);
            return Err(sys.log_err(&msg));
        }
    }
    if let Some(dir) = &op.chdir
        && let Err(e) = std::env::set_current_dir(quoting::os_from_bytes(dir))
    {
        let msg = join(&[
            &op.tty,
            b": can't change working directory ",
            dir,
            b": ",
            errmsg::strerror(&e).as_bytes(),
        ]);
        return Err(sys.log_err(&msg));
    }
    if op.nice != 0
        && let Err(e) = libcall::process::nice(op.nice)
    {
        let msg = cat(
            &op.tty,
            format!(": can't change process priority: {}", strerror(e)).as_bytes(),
        );
        sys.log_warn(&msg);
    }

    // "Let the login program take care of password validation."
    let errno = exec(&op.login, &argv);
    let program = argv.first().cloned().unwrap_or_default();
    let msg = join(&[
        &op.tty,
        b": can't exec ",
        &program,
        b": ",
        strerror(errno).as_bytes(),
    ]);
    Err(sys.log_err(&msg))
}

/// `execv (path, argv)`: the `errno` it failed with.
fn exec(path: &[u8], argv: &[Vec<u8>]) -> i32 {
    let Ok(path) = CString::new(path.to_vec()) else {
        return libcall::ENOENT;
    };
    // A word with a NUL cannot be one: every word came from the command line
    // or the name read, where a NUL ends or is not a byte of a word. None is
    // dropped.
    let words: Vec<CString> = argv
        .iter()
        .filter_map(|w| CString::new(w.clone()).ok())
        .collect();
    let refs: Vec<&std::ffi::CStr> = words.iter().map(CString::as_c_str).collect();
    let mut slots = vec![std::ptr::null(); refs.len().saturating_add(1)];
    libcall::process::execv(&path, &refs, &mut slots)
}

/// `open_tty (tty, tp, op)`: the line as standard input, output and error,
/// owned by root and the `tty` group, this process's controlling terminal
/// and its foreground; its modes as found; and `TERM` set for it. Whether
/// it is a virtual console goes into `op`.
fn open_tty(op: &mut Options, sys: &mut System) -> Result<Termios, Exit> {
    let tty = op.tty.clone();
    let pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    let mut closed = false;

    if op.tty_is_stdin {
        // "Standard input should already be connected to an open port. Make
        // sure it is open for read/write."
        let flags = tc::status_flags(0).unwrap_or(-1);
        if flags & libcall::fd::O_RDWR != libcall::fd::O_RDWR {
            return Err(sys.log_err(&cat(&tty, b": not open for read/write")));
        }
    } else {
        let gid = libcall::tty::group_id(c"tty").unwrap_or(0);
        let path = cat(b"/dev/", &tty);
        let cannot_open = |e: i32| {
            on_dev(
                &tty,
                &format!(": cannot open as standard input: {}", strerror(e)),
            )
        };
        // A path `snprintf` would cut is refused with whatever `errno` held:
        // nothing, here.
        let cpath = match CString::new(path.clone()) {
            Ok(p) if path.len() <= PATH_MAX => p,
            _ => return Err(sys.log_err(&cannot_open(0))),
        };
        let fd = libcall::fd::open_line(&cpath).map_err(|e| sys.log_err(&cannot_open(e)))?;

        // "Use root owner and group with permission -rw------- for the
        // period between getty and login."
        let owned = with_file(fd, |f| {
            std::os::unix::fs::fchown(f, Some(0), Some(gid))?;
            f.set_permissions(std::fs::Permissions::from_mode(if gid != 0 {
                0o620
            } else {
                0o600
            }))
        });
        if let Err(e) = owned {
            let msg = join(&[&path, b": ", errmsg::strerror(&e).as_bytes()]);
            if e.raw_os_error() == Some(libcall::EROFS) {
                // A line on a read-only filesystem keeps its owner.
                sys.log_warn(&msg);
            } else {
                return Err(sys.log_err(&msg));
            }
        }

        // "Sanity checks..."
        match with_file(fd, File::metadata) {
            Err(e) => {
                return Err(sys.log_err(&join(&[&path, b": ", errmsg::strerror(&e).as_bytes()])));
            }
            Ok(m) if !m.file_type().is_char_device() => {
                return Err(sys.log_err(&on_dev(&tty, ": not a character device")));
            }
            Ok(_) => {}
        }
        if !libcall::fd::is_terminal(fd) {
            return Err(sys.log_err(&on_dev(&tty, ": not a tty")));
        }
        take_controlling(fd, pid, &tty, sys);

        unchecked(libcall::fd::close(0));
        if op.flags & F_HANGUP != 0 {
            unchecked(libcall::tty::give_up_controlling(fd));
            // "Let's close all file descriptors before vhangup".
            unchecked(libcall::fd::close(fd));
            unchecked(libcall::fd::close(1));
            unchecked(libcall::fd::close(2));
            closed = true;
            if let Err(e) = libcall::tty::hang_up() {
                return Err(sys.log_err(&on_dev(
                    &tty,
                    &format!(": vhangup() failed: {}", strerror(e)),
                )));
            }
        } else {
            unchecked(libcall::fd::close(fd));
        }
        match libcall::fd::open_line(&cpath) {
            Ok(0) => {}
            Ok(other) => {
                unchecked(libcall::fd::close(other));
                return Err(sys.log_err(&cannot_open(0)));
            }
            Err(e) => return Err(sys.log_err(&cannot_open(e))),
        }
        take_controlling(0, pid, &tty, sys);
    }

    if let Err(e) = libcall::tty::set_foreground_group(0, pid) {
        sys.log_warn(&on_dev(
            &tty,
            &format!(": cannot set process group: {}", strerror(e)),
        ));
    }

    // "Get rid of the present outputs."
    if !closed {
        unchecked(libcall::fd::close(1));
        unchecked(libcall::fd::close(2));
    }
    // "set up stdout and stderr": plain copies, which `login` keeps across
    // its `exec` -- the second made only if the first landed on 1.
    let one = libcall::fd::dup(0);
    let two = (one == Ok(1)).then(|| libcall::fd::dup(0));
    if one != Ok(1) || two != Some(Ok(2)) {
        let e = one.err().or(two.and_then(Result::err)).unwrap_or(0);
        return Err(sys.log_err(&cat(
            &tty,
            format!(": dup problem: {}", strerror(e)).as_bytes(),
        )));
    }

    let tp = tc::get_attr(0).map_err(|e| {
        sys.log_err(&cat(
            &tty,
            format!(": failed to get terminal attributes: {}", strerror(e)).as_bytes(),
        ))
    })?;

    if op.term.is_none() {
        op.term = crate::ttys::getttynam_type(std::path::Path::new(PATH_TTYS), &tty);
    }

    // "Detect if this is a virtual console or serial/modem line."
    match libcall::tty::keyboard_mode(0) {
        Ok(mode) => {
            op.kbmode = mode;
            op.flags |= F_VCONSOLE;
            op.term.get_or_insert_with(|| DEFAULT_VCTERM.to_vec());
        }
        Err(_) => {
            op.kbmode = libcall::tty::K_RAW;
            op.term.get_or_insert_with(|| DEFAULT_STERM.to_vec());
        }
    }
    let term = op.term.clone().unwrap_or_default();
    // SAFETY: the process has one thread, so nothing reads the environment
    // while it changes.
    unsafe { std::env::set_var("TERM", quoting::os_from_bytes(&term)) };
    Ok(tp)
}

/// Make `fd` the controlling terminal unless it is this session's already
/// -- taking it from another session if need be.
fn take_controlling(fd: i32, pid: i32, tty: &[u8], sys: &mut System) {
    if libcall::tty::session_of(fd) != Ok(pid)
        && let Err(e) = libcall::tty::make_controlling(fd, true)
    {
        sys.log_warn(&on_dev(
            tty,
            &format!(": cannot get controlling tty: {}", strerror(e)),
        ));
    }
}

/// `termio_init (op, tp)`: a virtual console made sane in the locale its
/// keyboard types, and cleared; a serial line made raw at its first speed.
fn termio_init(op: &mut Options, tp: &mut Termios, sys: &mut System) {
    // A lock an earlier program put on the line's modes would keep them
    // through every `tcsetattr` below. Upstream first waits for plymouth
    // to let go of one (no plymouth here: deliberate difference 1); the
    // unlock itself fails unprivileged, and goes on regardless, as there.
    unchecked(libcall::tty::unlock_modes(0));

    if op.flags & F_VCONSOLE != 0 {
        // A locale that is not there leaves the one selected, as upstream's
        // unchecked `setlocale` does.
        if op.kbmode == libcall::tty::K_UNICODE {
            let _ = libcall::locale::select(libcall::locale::LC_CTYPE, c"C.UTF-8");
            op.flags |= F_UTF8;
        } else {
            let _ = libcall::locale::select(libcall::locale::LC_CTYPE, c"POSIX");
            op.flags &= !F_UTF8;
        }
        reset_vc(op, tp, false, sys);
        if termio::eight_bit_clean(tp) {
            op.flags |= F_EIGHTBITS;
        }
        if op.flags & F_NOCLEAR == 0 {
            termio_clear();
        }
        return;
    }
    termio::init_serial(op, tp);
    // "Check for terminal size and if not found set default".
    if let Ok(mut ws) = libcall::pty::window_size(0) {
        if ws.rows == 0 {
            ws.rows = 24;
        }
        if ws.cols == 0 {
            ws.cols = 80;
        }
        unchecked(libcall::pty::set_window_size(0, ws));
    }
    // "Flush input and output queues, important for modems!"
    unchecked(tc::flush(0, tc::TCIOFLUSH));
    if let Err(e) = tc::set_attr(0, tc::TCSANOW, tp) {
        sys.log_warn(format!("setting terminal attributes failed: {}", strerror(e)).as_bytes());
    }
    // "Go to blocking input even in local mode."
    unchecked(tc::clear_nonblocking(0));
}

/// `termio_clear (STDOUT_FILENO)`: the cursor home, the screen below it
/// erased, the scrolling region the whole screen -- not a full reset,
/// which would lose a console's Unicode mode.
fn termio_clear() {
    out(b"\x1b[r\x1b[H\x1b[J");
}

/// `reset_vc (op, tp, canon)`: a virtual console's sane modes, set -- not
/// canonical, until the name is read.
fn reset_vc(op: &Options, tp: &mut Termios, canon: bool, sys: &mut System) {
    termio::reset_virtual_console(tp, op.flags & F_KEEPCFLAGS != 0, op.flags & F_UTF8 != 0);
    // "Discard all the flags that makes the line go canonical with echoing.
    // We need to know when the user starts typing."
    if !canon {
        tp.c_lflag = 0;
    }
    if let Err(e) = tc::set_attr(0, tc::TCSADRAIN, tp) {
        sys.log_warn(format!("setting terminal attributes failed: {}", strerror(e)).as_bytes());
    }
    unchecked(tc::clear_nonblocking(0));
}

/// `auto_baud (tp)`: the speed a modem's status message names, set --
/// what it said in its first second, read seven bits wide.
fn auto_baud(tp: &mut Termios) {
    let iflag = tp.c_iflag;
    tp.c_iflag |= tc::ISTRIP;
    let vmin = tp.c_cc.get(tc::VMIN).copied().unwrap_or(1);
    if let Some(c) = tp.c_cc.get_mut(tc::VMIN) {
        *c = 0;
    }
    unchecked(tc::set_attr(0, tc::TCSANOW, tp));
    unchecked(libcall::clock::nanosleep(1, 0));
    let mut buf = vec![0u8; 8191];
    if let Ok(n) = libcall::fd::read(0, &mut buf)
        && n > 0
        && let Some(speed) = termio::status_message_speed(buf.get(..n).unwrap_or_default())
    {
        unchecked(tc::set_input_speed(tp, speed));
        unchecked(tc::set_output_speed(tp, speed));
    }
    tp.c_iflag = iflag;
    if let Some(c) = tp.c_cc.get_mut(tc::VMIN) {
        *c = vmin;
    }
    unchecked(tc::set_attr(0, tc::TCSANOW, tp));
}

/// `next_speed (op, tp)`: the next speed of the list, set -- the first
/// again after the last.
fn next_speed(op: &Options, tp: &mut Termios, speeds: &mut Speeds) {
    let n = op.speeds.len().max(1);
    let index = match speeds.index {
        // "If the F_KEEPSPEED flags is set then the FIRST_SPEED is not
        // tested yet".
        None if op.flags & F_KEEPSPEED != 0 => 0,
        None => 1usize.checked_rem(n).unwrap_or(0),
        Some(i) => i.saturating_add(1).checked_rem(n).unwrap_or(0),
    };
    speeds.index = Some(index);
    let speed = op
        .speeds
        .get(index)
        .copied()
        .unwrap_or(libcall::termios::B9600);
    unchecked(tc::set_input_speed(tp, speed));
    unchecked(tc::set_output_speed(tp, speed));
    unchecked(tc::set_attr(0, tc::TCSANOW, tp));
}

/// `eval_issue_file (ie, op, tp)`, and then -- the issue showing an
/// address -- the netlink socket that hears it change.
fn eval_issue(ie: &mut Issue, op: &Options, tp: &Termios, reload: &mut Reload, sys: &mut System) {
    let paths = Paths::default();
    Eval::new(op, tc::input_speed(tp), &paths, &SystemHost, sys).eval(ie);
    if ie.netlink_groups != 0 {
        reload.open_netlink(ie.netlink_groups);
    }
}

/// `inotify_fd`'s three states.
enum Watch {
    /// `AGETTY_RELOAD_FDNONE`: the reload file is not there yet; it is
    /// made, and watched, at the next wait.
    Untried,
    /// The file is there and the watch could not be made.
    Failed,
    /// The file is watched.
    On(libcall::inotify::Inotify),
}

/// What tells a waiting getty to show the issue again: a touch of
/// [`RELOAD_FILE`], and a change of an address the issue shows.
struct Reload {
    watch: Watch,
    /// `netlink_fd`.
    netlink: Option<i32>,
}

impl Reload {
    /// `open_netlink ()`: listen for `groups`' address changes, unless a
    /// socket is open already. One that could not be made, or that an end
    /// or an error closed, is tried again at the next evaluation of the
    /// issue, as upstream's is.
    fn open_netlink(&mut self, groups: u32) {
        if self.netlink.is_none() {
            // A socket that cannot be made is changes not heard: upstream
            // goes on without one too.
            self.netlink = libcall::netlink::open_route(groups).ok();
        }
    }

    /// Stop listening: `close (netlink_fd)`.
    fn close_netlink(&mut self) {
        if let Some(fd) = self.netlink.take() {
            unchecked(libcall::fd::close(fd));
        }
    }

    /// `process_netlink ()`: read what the socket has said, without
    /// waiting: whether an address changed. An end or an error closes it.
    fn process_netlink(&mut self) -> bool {
        let mut triggered = false;
        while let Some(fd) = self.netlink {
            let mut buf = [0u8; 4096];
            match libcall::netlink::receive(fd, &mut buf) {
                Ok(0) => break,
                Ok(n) => match libcall::netlink::first_message(buf.get(..n).unwrap_or_default()) {
                    libcall::netlink::Said::End => {
                        self.close_netlink();
                        break;
                    }
                    libcall::netlink::Said::Change => triggered = true,
                    libcall::netlink::Said::Nothing => {}
                },
                // "Failure, just stop listening for changes".
                Err(_) => {
                    self.close_netlink();
                    break;
                }
            }
        }
        triggered
    }

    /// `wait_for_term_input (fd)`: wait for something to read on `fd` --
    /// `true` -- or for a reason to show the issue again: `false`. The
    /// reload file is made and watched the first time; one that cannot be
    /// made is warned of, and tried again, every time.
    fn wait_for_term_input(&mut self, fd: i32, log: &mut dyn Diag) -> bool {
        if matches!(self.watch, Watch::Untried) {
            match libcall::fd::open_create_read(RELOAD_FILE, 0o600) {
                Ok(file) => {
                    self.watch = match libcall::inotify::Inotify::new() {
                        Ok(watch) => {
                            let mask = libcall::inotify::IN_ATTRIB | libcall::inotify::IN_MODIFY;
                            unchecked(watch.watch(RELOAD_FILE, mask));
                            Watch::On(watch)
                        }
                        Err(_) => Watch::Failed,
                    };
                    unchecked(libcall::fd::close(file));
                }
                Err(e) => log.log_warn(&join(&[
                    b"failed to create reload file: ",
                    RELOAD_FILE.to_bytes(),
                    b": ",
                    strerror(e).as_bytes(),
                ])),
            }
        }
        loop {
            let mut fds = vec![fd];
            let watch_at = match &self.watch {
                Watch::On(watch) => {
                    let at = fds.len();
                    fds.push(watch.fd());
                    Some(at)
                }
                _ => None,
            };
            let netlink_at = self.netlink.map(|nl| {
                let at = fds.len();
                fds.push(nl);
                at
            });
            // "If waiting fails, just fall through, presumably reading input
            // will fail".
            let Ok(ready) = libcall::fd::wait_readable_among(&fds, -1) else {
                return true;
            };
            let is_ready = |at: Option<usize>| at.is_some_and(|i| ready & (1 << i) != 0);
            if ready & 1 != 0 {
                return true;
            }
            if is_ready(netlink_at) {
                if !self.process_netlink() {
                    continue;
                }
            } else if is_ready(watch_at)
                && let Watch::On(watch) = &self.watch
            {
                // "Just drain the inotify buffer".
                let mut buf = [0u8; 512];
                while watch
                    .read(&mut buf)
                    .is_ok_and(|mut events| events.next().is_some())
                {}
            }
            return false;
        }
    }
}

/// `print_issue_file (ie, op, tp)`: a new line first, unless
/// `--nonewline`; then the issue evaluated last -- once, as upstream
/// prints it once per evaluation -- with the output's new lines made
/// carriage return and new line while it goes, when `issue.d` added to it.
fn print_issue(ie: &mut Issue, op: &Options, tp: &mut Termios) {
    if op.flags & F_NONL == 0 {
        out(b"\r\n");
    }
    if ie.do_tcsetattr && op.flags & F_VCONSOLE == 0 {
        tp.c_oflag |= tc::ONLCR | tc::OPOST;
        unchecked(tc::set_attr(0, tc::TCSADRAIN, tp));
    }
    if let Some(mem) = &ie.mem
        && !mem.is_empty()
    {
        out(mem);
    }
    // What was printed, to tell a reload's issue from.
    ie.mem_old = ie.mem.take();
}

/// The screen cleared before the issue is shown again, on a virtual
/// console that is cleared at all.
fn clear_for_reload(op: &Options) {
    if op.flags & F_VCONSOLE != 0 && op.flags & F_NOCLEAR == 0 {
        termio_clear();
    }
}

/// `do_prompt (ie, op, tp)`: the issue, the pause for Enter under
/// `--login-pause` -- the issue shown again if a reload changes it while
/// waiting -- a virtual console's keyboard hint, the host's name and
/// `login: `.
fn do_prompt(
    ie: &mut Issue,
    op: &Options,
    tp: &mut Termios,
    reload: &mut Reload,
    sys: &mut System,
) {
    loop {
        print_issue(ie, op, tp);
        if op.flags & F_LOGINPAUSE != 0 {
            out(b"[press ENTER to login]\n");
            if !reload.wait_for_term_input(0, sys) {
                eval_issue(ie, op, tp, reload, sys);
                if issue_is_changed(ie) {
                    clear_for_reload(op);
                    continue;
                }
            }
            // `getc (stdin)`: a buffer's worth is read, and what follows
            // the first byte is lost with the buffer.
            let mut buf = [0u8; 1024];
            unchecked(libcall::fd::read(0, &mut buf));
        }
        break;
    }
    if op.flags & F_NOHINTS == 0
        && op.autolog.is_none()
        && op.flags & F_VCONSOLE != 0
        && let Ok(kb) = libcall::tty::keyboard_leds(0)
    {
        let numlock = std::path::Path::new(crate::prompt::NUMLOCK_ON).exists();
        if let Some(hint) = crate::prompt::keyboard_hint(kb, numlock) {
            out(&hint);
        }
    }
    if let Some(name) = crate::prompt::host_name(op, &SystemHost) {
        out(&name);
    }
    if op.autolog.is_none() {
        out(crate::prompt::LOGIN_PROMPT);
    }
}

/// `get_logname (ie, op, tp, cp)`: the name -- prompted for again after an
/// empty one, and after a reload that changed the issue while nothing was
/// typed -- or `None` for a BREAK that asks for the next speed.
fn get_logname(
    ie: &mut Issue,
    op: &Options,
    tp: &mut Termios,
    cp: &mut Chardata,
    reload: &mut Reload,
    sys: &mut System,
) -> Result<Option<Vec<u8>>, Exit> {
    *cp = Chardata::default();
    // "Flush pending input (especially important after parsing or
    // switching the baud rate)."
    if op.flags & F_VCONSOLE == 0 {
        unchecked(libcall::clock::nanosleep(1, 0));
    }
    unchecked(tc::flush(0, tc::TCIFLUSH));
    eval_issue(ie, op, tp, reload, sys);
    loop {
        do_prompt(ie, op, tp, reload, sys);
        loop {
            let echo = tp.c_lflag & tc::ECHO != 0;
            let mut line = Line {
                reload: &mut *reload,
                log: sys.clone(),
            };
            match read_name(&mut line, op, echo, cp, sys)? {
                Read::NextSpeed => return Ok(None),
                Read::Reload => {
                    // "refresh prompt -- discard input data, clear terminal
                    // and call do_prompt() again".
                    if op.flags & F_VCONSOLE == 0 {
                        unchecked(libcall::clock::nanosleep(1, 0));
                    }
                    eval_issue(ie, op, tp, reload, sys);
                    if !issue_is_changed(ie) {
                        continue;
                    }
                    unchecked(tc::flush(0, tc::TCIFLUSH));
                    clear_for_reload(op);
                    break;
                }
                Read::Name(name) if name.is_empty() => break,
                Read::Name(mut name) => {
                    finish_name(&mut name, op, cp, sys)?;
                    return Ok(Some(name));
                }
            }
        }
    }
}

/// `termio_final (op, tp, cp)`: the serial line's modes for the login, set.
fn termio_final(
    op: &Options,
    tp: &mut Termios,
    cp: &Chardata,
    sys: &mut System,
) -> Result<(), Exit> {
    termio::final_modes(op, tp, cp);
    tc::set_attr(0, tc::TCSANOW, tp).map_err(|e| {
        sys.log_err(&cat(
            &op.tty,
            format!(": failed to set terminal attributes: {}", strerror(e)).as_bytes(),
        ))
    })
}
