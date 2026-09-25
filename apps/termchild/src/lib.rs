//! The program on the far side of a terminal.
//!
//! Shared by `apps/terminal`, where it began as `child.rs`, and
//! `apps/tmux`, whose every pane is a terminal of its own: one link a pane.
//!
//! A terminal emulator is the *master* end of a pseudo-terminal. It writes
//! what the user types into the master and draws what comes back out of it.
//! Everything in between -- echo, line editing in cooked mode, `^C` becoming
//! `SIGINT` for the foreground job, `SIGWINCH` when the window changes size --
//! is the kernel's line discipline (`kernel/src/tty/pty.rs`), and the shell on
//! the slave end is interactive because `isatty(0)` says it is on a terminal.
//!
//! [`Link`] is that connection as the emulator sees it: bytes out, bytes in, a
//! size, and an exit. [`spawn_shell`] makes a real one, on a kernel
//! pseudo-terminal, through `libcall::pty` (design-decisions §768 says why
//! that and not `posix`).
//!
//! # What this replaced
//!
//! `pty.rs`: two thousand lines modelling a pseudo-terminal *inside the
//! terminal's own process* -- a master, a slave, a cooked-mode line discipline,
//! a window size -- with a shell attached to the model's slave by two threads
//! copying bytes to and from the shell's **pipes**. The shell was therefore
//! never on a terminal: `isatty(0)` was false, so it printed no prompt and did
//! no job control; `^C` became a variant of a Rust enum that no process ever
//! received; the window size went into a struct nobody could ask; full-screen
//! programs refused to start; and the shell's standard error was a pipe whose
//! reading end had already been dropped, so its first complaint raised
//! `SIGPIPE` and killed it. On the development host `/bin/sh` did not exist
//! and the model echoed keystrokes into nowhere, which is what the emulator's
//! tests had been measuring. The kernel has had real pseudo-terminals since
//! 2026-08-23 (syscalls 544-556); the model was a second line discipline
//! beside the real one, in the one process that must not have one.
//!
//! # Being woken
//!
//! A link wakes the application when there is something to read: give it the
//! window loop's `Waker` with [`Link::set_waker`], and its reader thread wakes
//! the loop after every chunk the child writes, while a waiter thread wakes it
//! when the child finishes. So a terminal at a prompt sleeps until the shell
//! says something, and the first keystroke after a pause is echoed as soon as
//! the shell echoes it.
//!
//! It used to be asked on a clock -- twenty times a second at a prompt,
//! forever, to find nothing -- because the window loop woke an application
//! for the compositor's events and its own timer and for nothing else. Lane F
//! added the waker (`oswindow::app::App::attach_waker`), asked for in
//! `requests/e-f-wake-an-application-for-its-own-descriptor.md`. A link given
//! no waker, or one that cannot wake anyone, says so, and the terminal goes on
//! asking it on a clock.

use std::fmt;
use std::path::PathBuf;
use std::task::Waker;

use libcall::pty::WinSize;

/// How a child's life ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It called `exit` with this status.
    Code(i32),
    /// This signal killed it.
    Signal(i32),
    /// It finished, and its exit status could not be collected (`waitpid`
    /// failed, with this `errno`). Said as itself rather than guessed at.
    Lost(i32),
}

impl Exit {
    /// Whether the child finished cleanly -- `exit 0`, the shell's answer to
    /// the user typing `exit`.
    #[must_use]
    pub fn is_clean(self) -> bool {
        self == Self::Code(0)
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Code(code) => write!(f, "exited with status {code}"),
            Self::Signal(sig) => match signal_name(sig) {
                Some(name) => write!(f, "was killed by {name} (signal {sig})"),
                None => write!(f, "was killed by signal {sig}"),
            },
            Self::Lost(errno) => write!(
                f,
                "finished, and its exit status could not be collected ({})",
                describe_errno(errno)
            ),
        }
    }
}

/// The names of the signals a user is likely to see end a shell.
fn signal_name(sig: i32) -> Option<&'static str> {
    Some(match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return None,
    })
}

/// A sentence for the `errno` values starting a program can produce.
///
/// The numbers are `libcall`'s, which are checked against `posix`'s there; the
/// fall-back names the number rather than inventing a meaning for it.
#[must_use]
pub fn describe_errno(errno: i32) -> String {
    use libcall::pty::{
        E2BIG, EACCES, EAGAIN, ECHILD, EIO, ELOOP, EMFILE, ENFILE, ENOEXEC, ENOMEM, ENOTDIR,
        ETXTBSY,
    };
    let text = match errno {
        libcall::ENOENT => "no such file",
        EACCES => "permission denied",
        ENOEXEC => "not a program this system can run",
        ENOTDIR => "a part of the path is not a directory",
        ELOOP => "too many symbolic links",
        ETXTBSY => "the program is open for writing",
        E2BIG => "too many arguments or environment variables",
        ENOMEM => "out of memory",
        EAGAIN => "out of processes, for now",
        EMFILE => "this program has run out of file descriptors",
        ENFILE => "the system has run out of file descriptors or terminals",
        ECHILD => "no such child process",
        EIO => "input/output error",
        libcall::ENAMETOOLONG => "the path is too long",
        libcall::ENOSYS => "this system has no pseudo-terminals",
        _ => return format!("error {errno}"),
    };
    format!("{text} (error {errno})")
}

/// The connection between the emulator and the program it runs.
///
/// None of these may block: they are called from the event handler, and a
/// terminal whose window stops answering because its child stopped reading is
/// a terminal the user cannot close.
pub trait Link {
    /// Offer `bytes` to the child, returning how many it took.
    ///
    /// The rest are the caller's to offer again later, not lost: a paste into
    /// a program that is busy arrives late rather than with its middle missing.
    fn send(&mut self, bytes: &[u8]) -> usize;

    /// Append to `into` what the child has written since the last call, up to
    /// about `limit` bytes -- whole chunks as the child wrote them, so the
    /// last may carry it past the limit by one chunk.
    ///
    /// Bounded because the emulator parses everything it is handed before it
    /// can draw, and a child that writes faster than that -- `cat` of a large
    /// file -- must not hold the window for the whole of its output. What is
    /// left waits for the next call, and the child waits behind it: the link
    /// holds only a bounded amount, so a flood is paced by the kernel's buffer
    /// rather than by this process's memory.
    fn receive(&mut self, into: &mut Vec<u8>, limit: usize);

    /// The terminal is now `size`. The child is told, and a full-screen
    /// program redraws.
    fn resize(&mut self, size: WinSize);

    /// Whether the child has finished, and how.
    ///
    /// Reported exactly once. `None` before the child has finished and after
    /// the report, so a caller that polls on every tick announces an exit
    /// once without keeping a flag of its own to remember it by.
    fn poll_exit(&mut self) -> Option<Exit>;

    /// The terminal is going away: tell the child. Idempotent.
    fn hang_up(&mut self);

    /// Wake the application through `waker` whenever there is output to read
    /// or the child has finished, rather than waiting to be asked.
    ///
    /// Returns whether it will. A link that cannot wake anyone keeps the
    /// default and answers `false`, and the terminal goes on asking it on a
    /// clock -- the one thing that must not happen is a terminal that stops
    /// asking a link that never wakes it.
    fn set_waker(&mut self, waker: Waker) -> bool {
        let _ = waker;
        false
    }
}

/// Why no shell could be started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnError {
    /// The program that was to be run.
    pub program: PathBuf,
    /// Why not, as an `errno`.
    pub errno: i32,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.errno == libcall::ENOSYS {
            // Not the program's fault, so not the program's name first.
            write!(
                f,
                "This system has no pseudo-terminals, so no shell can run here."
            )
        } else {
            write!(
                f,
                "{} could not be started: {}.",
                self.program.display(),
                describe_errno(self.errno)
            )
        }
    }
}

/// The shell to run: `$SHELL` if it names a path from the root, else
/// `/bin/sh`.
///
/// `$SHELL` is read, rather than a fixed path used, because it is how the user
/// says which shell they want -- `login` sets it from the account database.
/// A relative value is ignored rather than resolved: `execve` does not search
/// `PATH`, and resolving it here against the terminal's own working directory
/// would run whatever file of that name happened to be there.
///
/// `has_root` rather than `is_absolute`: on SlateOS the two agree, and on a
/// Windows host -- where this is compiled for the tests -- a path like
/// `/usr/bin/fish` has a root and no drive, so `is_absolute` would call the
/// SlateOS path relative and the tests would be checking a different rule.
#[must_use]
pub fn shell_path(shell_var: Option<std::ffi::OsString>) -> PathBuf {
    shell_var
        .map(PathBuf::from)
        .filter(|p| p.has_root())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

/// The environment the shell gets: the terminal's own, with the terminal's
/// identity set and the stale sizes removed.
///
/// * `TERM=xterm-256color`, because that is the escape-sequence dialect this
///   emulator speaks -- 256 colours and the xterm private modes -- and a
///   program that is not told draws for a lowest-common-denominator terminal
///   or refuses to draw at all.
/// * `COLORTERM=truecolor`, because it also takes 24-bit colour
///   (`parse_extended_color`), which `TERM` has no way to say.
/// * `COLUMNS` and `LINES` are removed. They describe whatever terminal
///   started *this* program, and a shell that inherits them believes a size
///   that the window's real one contradicts; the real size arrives through
///   the terminal itself.
///
/// Entries are bytes, never forced through UTF-8: an environment is
/// OS-boundary data and may hold any byte but NUL.
#[must_use]
pub fn shell_environment<I>(inherited: I) -> Vec<Vec<u8>>
where
    I: IntoIterator<Item = (Vec<u8>, Vec<u8>)>,
{
    const OVERRIDDEN: [&[u8]; 4] = [b"TERM", b"COLORTERM", b"COLUMNS", b"LINES"];
    let mut env: Vec<Vec<u8>> = inherited
        .into_iter()
        .filter(|(name, _)| !OVERRIDDEN.contains(&name.as_slice()))
        // A name containing `=` cannot be expressed as a `NAME=value` entry
        // at all, and a NUL anywhere cannot cross the C boundary; neither can
        // come from a real environment, and either would corrupt the entry.
        .filter(|(name, value)| !name.contains(&b'=') && !name.contains(&0) && !value.contains(&0))
        .map(|(mut name, value)| {
            name.push(b'=');
            name.extend_from_slice(&value);
            name
        })
        .collect();
    env.push(b"TERM=xterm-256color".to_vec());
    env.push(b"COLORTERM=truecolor".to_vec());
    env
}

/// Start the user's shell on a new pseudo-terminal of `size`.
///
/// # Errors
///
/// The program that could not be run and why -- see [`SpawnError`]'s
/// `Display`, which is written to be put on the screen as it is.
#[cfg(unix)]
pub fn spawn_shell(size: WinSize) -> Result<Box<dyn Link>, SpawnError> {
    use std::ffi::CString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let program = shell_path(std::env::var_os("SHELL"));
    let refuse = |errno| SpawnError {
        program: program.clone(),
        errno,
    };

    let path = CString::new(program.as_os_str().as_bytes()).map_err(|_| refuse(libcall::EINVAL))?;
    // A non-login interactive shell, as every graphical terminal starts: the
    // login shell already ran when the user logged in, and running its
    // profile again in every window would repeat whatever it does once.
    let name = program.file_name().map_or_else(
        || program.as_os_str().as_bytes().to_vec(),
        |n| n.as_bytes().to_vec(),
    );
    let argv0 = CString::new(name).map_err(|_| refuse(libcall::EINVAL))?;

    let env: Vec<CString> =
        shell_environment(std::env::vars_os().map(|(k, v)| (k.into_vec(), v.into_vec())))
            .into_iter()
            // `shell_environment` removed every entry with a NUL, so this cannot
            // fail; filtering rather than unwrapping keeps a mistake there from
            // becoming a panic here.
            .filter_map(|entry| CString::new(entry).ok())
            .collect();
    let envp: Vec<&std::ffi::CStr> = env.iter().map(CString::as_c_str).collect();

    let link = pty_link::PtyLink::spawn(&path, &[argv0.as_c_str()], &envp, size).map_err(refuse)?;
    Ok(Box::new(link))
}

/// Start the user's shell on a new pseudo-terminal of `size`.
///
/// # Errors
///
/// Always: this build is for a host with no pseudo-terminals. The answer comes
/// from `libcall`, the same place the target build's comes from, so that the
/// terminal cannot say something different from what the library would.
#[cfg(not(unix))]
pub fn spawn_shell(size: WinSize) -> Result<Box<dyn Link>, SpawnError> {
    let errno = match libcall::pty::spawn(c"/bin/sh", &[c"sh"], &[], size) {
        Err(errno) => errno,
        // The library's host arm cannot succeed. Were it ever to, this build
        // has no link to drive the child with, which is the same answer.
        Ok(_) => libcall::ENOSYS,
    };
    Err(SpawnError {
        program: PathBuf::from("/bin/sh"),
        errno,
    })
}

/// The real link: a kernel pseudo-terminal and the process on it.
#[cfg(unix)]
mod pty_link {
    use super::{Exit, Link};
    use libcall::pty::{ChildState, WinSize};
    use std::ffi::CStr;
    use std::fs::File;
    use std::io::{ErrorKind, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::task::Waker;
    use std::time::{Duration, Instant};

    /// The waker a link was given, shared with its threads.
    type WakeSlot = Arc<Mutex<Option<Waker>>>;

    /// Wake whoever is waiting, if anyone is.
    ///
    /// A poisoned slot -- a thread panicked holding it -- still holds a waker,
    /// and waking through it is still right.
    fn wake(slot: &WakeSlot) {
        let guard = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(waker) = guard.as_ref() {
            waker.wake_by_ref();
        }
    }

    /// Who is left to collect the child: the link, until it is dropped, and
    /// then the waiter thread.
    ///
    /// The one piece of state both touch. Collecting a child frees its process
    /// id, and the link *signals* that id when it hangs up -- so the two must
    /// never both think they may do it, or a hang-up could reach whatever
    /// process was given the id next.
    #[derive(Default)]
    struct Collect {
        /// The link has been dropped: nobody will ask `poll_exit` again.
        link_gone: bool,
        /// The waiter has seen the child finish (it is waitable, not yet
        /// collected).
        finished: bool,
    }

    /// How long after the child is reaped its remaining output may still be
    /// arriving before the exit is announced anyway.
    ///
    /// The exit is announced once the output side has closed, so that the
    /// last lines a program printed come *before* the note that it has
    /// finished rather than after. But the output side stays open for as long
    /// as anything holds the slave -- a background job the shell started, say
    /// -- and a terminal that waited for that would sit at a dead shell for as
    /// long as the job ran.
    const OUTPUT_GRACE: Duration = Duration::from_millis(250);

    /// How many chunks of output the reader may hold for the emulator: at
    /// most a megabyte, at the reader's sixteen-kilobyte reads.
    const READ_QUEUE_CHUNKS: usize = 64;

    /// What the reader thread passes back.
    enum FromChild {
        Bytes(Vec<u8>),
        /// The master reported end of file or `EIO`: no process holds the
        /// slave any more, and nothing further will arrive.
        Closed,
    }

    pub struct PtyLink {
        pid: i32,
        /// Held for `resize` and to keep the terminal open; the two threads
        /// each hold their own duplicate.
        master: File,
        /// `None` once the writer has gone -- after a hang-up, or when a write
        /// failed because the child is gone.
        to_writer: Option<Sender<Vec<u8>>>,
        from_reader: Receiver<FromChild>,
        output_closed: bool,
        /// The exit, once `waitpid` has collected it, and when.
        reaped: Option<(Exit, Instant)>,
        reported: bool,
        hung_up: bool,
        /// The waker the reader and the waiter wake, once there is one.
        wake: WakeSlot,
        /// Shared with the waiter thread: see [`Collect`].
        collect: Arc<Mutex<Collect>>,
    }

    impl PtyLink {
        /// Start `path` on a new terminal and connect to it.
        ///
        /// The two threads are started *after* the fork, so the fork happens in
        /// a process that has only the threads it had before -- which, for the
        /// first shell a terminal starts, is one.
        pub fn spawn(
            path: &CStr,
            argv: &[&CStr],
            envp: &[&CStr],
            size: WinSize,
        ) -> Result<Self, i32> {
            let spawned = libcall::pty::spawn(path, argv, envp, size)?;
            // SAFETY: `spawn` opened `spawned.master` for this call and handed
            // it over; nothing else in the process owns it, so this `File` is
            // its one owner and closes it exactly once.
            let master = unsafe { File::from_raw_fd(spawned.master) };

            // Everything that can fail from here on leaves a running child
            // behind, and a child nobody is connected to is a process leaked.
            let give_up = |e: std::io::Error| {
                let errno = e.raw_os_error().unwrap_or(libcall::pty::EIO);
                // The child is ours and has had no chance to do anything yet;
                // whether this succeeds or finds it already gone, there is
                // nothing further to do about it, and the reap below collects
                // it either way.
                let _ = libcall::kill(spawned.pid, libcall::SIGKILL);
                reap_blocking(spawned.pid);
                errno
            };

            let reader = master.try_clone().map_err(give_up)?;
            let writer = master.try_clone().map_err(give_up)?;

            // Bounded: when the emulator falls behind, the reader blocks, the
            // kernel's buffer fills, and the child's writes block -- ordinary
            // flow control. Unbounded, a child printing without end would fill
            // this process's memory instead.
            let (from_child_tx, from_reader) = mpsc::sync_channel(READ_QUEUE_CHUNKS);
            let wake_slot: WakeSlot = Arc::default();
            let reader_wake = Arc::clone(&wake_slot);
            std::thread::Builder::new()
                .name("terminal-pty-reader".into())
                .spawn(move || read_child(reader, &from_child_tx, &reader_wake))
                .map_err(give_up)?;

            let (to_writer, to_child_rx) = mpsc::channel::<Vec<u8>>();
            std::thread::Builder::new()
                .name("terminal-pty-writer".into())
                .spawn(move || write_child(writer, &to_child_rx))
                .map_err(give_up)?;

            let collect: Arc<Mutex<Collect>> = Arc::default();
            let (pid, waiter_wake, waiter_collect) =
                (spawned.pid, Arc::clone(&wake_slot), Arc::clone(&collect));
            std::thread::Builder::new()
                .name("terminal-pty-waiter".into())
                .spawn(move || wait_child(pid, &waiter_wake, &waiter_collect))
                .map_err(give_up)?;

            Ok(Self {
                pid: spawned.pid,
                master,
                to_writer: Some(to_writer),
                from_reader,
                output_closed: false,
                reaped: None,
                reported: false,
                hung_up: false,
                wake: wake_slot,
                collect,
            })
        }
    }

    /// The reader thread: everything the child writes, until nothing holds
    /// the slave -- waking the application after each chunk.
    fn read_child(mut master: File, out: &SyncSender<FromChild>, wake_slot: &WakeSlot) {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match master.read(&mut buf) {
                // End of file, or `EIO` below: the last slave descriptor has
                // closed, which is how a master reports the end of a session.
                Ok(0) => break,
                Ok(n) => {
                    let chunk = buf.get(..n).unwrap_or_default().to_vec();
                    if out.send(FromChild::Bytes(chunk)).is_err() {
                        // The emulator has dropped the link; nobody is reading.
                        return;
                    }
                    wake(wake_slot);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        // The receiver may already be gone if the link was dropped while this
        // was reading, and then there is nobody left to tell.
        let _ = out.send(FromChild::Closed);
        wake(wake_slot);
    }

    /// The waiter thread: notice the child finishing, and say so.
    ///
    /// A thread of its own because the end of the output is not the end of
    /// the child -- a job the shell started in the background can hold the
    /// terminal open long after the shell has exited -- and a terminal that is
    /// woken rather than asking on a clock would otherwise never learn it.
    ///
    /// It does not collect the child: `wait_exited` leaves it waitable, so its
    /// process id stays reserved until the link's `poll_exit` collects it, and
    /// the link's hang-up can never signal an id that has been handed on. The
    /// exception is a link that has already been dropped -- a closed pane --
    /// which will never ask again; then this collects it, rather than leaving
    /// a finished process behind for as long as the application runs.
    fn wait_child(pid: i32, wake_slot: &WakeSlot, collect: &Mutex<Collect>) {
        // An error is a child that cannot be waited for, which `poll_exit`
        // will find out for itself and report; either way this is done.
        let _ = libcall::pty::wait_exited(pid);
        {
            let mut state = collect
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.finished = true;
            if state.link_gone {
                // Nobody left to ask, and the id is still ours: collect it.
                let _ = libcall::pty::try_wait(pid);
                return;
            }
        }
        wake(wake_slot);
        // The exit is announced once the output side closes -- which wakes by
        // itself -- or after `OUTPUT_GRACE`, which nothing else would wake for.
        std::thread::sleep(OUTPUT_GRACE);
        wake(wake_slot);
    }

    /// The writer thread: what the user typed, in order, for as long as the
    /// child is there to take it.
    ///
    /// A thread rather than a write from the event handler because a write to
    /// a terminal blocks when the child is not reading -- a paste into a
    /// program busy computing -- and a blocked handler is a window that cannot
    /// even be closed.
    fn write_child(mut master: File, input: &Receiver<Vec<u8>>) {
        while let Ok(chunk) = input.recv() {
            if master.write_all(&chunk).is_err() {
                // The child is gone (`EIO`). What is still queued has nowhere
                // to go, and the exit is reported by `poll_exit`.
                return;
            }
        }
    }

    /// Wait for `pid` and discard its status. Only for a child just killed.
    fn reap_blocking(pid: i32) {
        let start = Instant::now();
        while let Ok(ChildState::Running) = libcall::pty::try_wait(pid) {
            if start.elapsed() >= Duration::from_secs(1) {
                // It has been sent SIGKILL, which cannot be caught, so it will
                // die; it is left for init to collect rather than blocking the
                // window on it any longer.
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    impl Link for PtyLink {
        fn send(&mut self, bytes: &[u8]) -> usize {
            if let Some(tx) = &self.to_writer
                && tx.send(bytes.to_vec()).is_err()
            {
                // The writer stopped because the child is gone.
                self.to_writer = None;
            }
            // Taken either way. Once the writer has gone nothing will ever read
            // these bytes, and keeping them queued would only make the terminal
            // offer them again on every tick until the exit is announced.
            bytes.len()
        }

        fn receive(&mut self, into: &mut Vec<u8>, limit: usize) {
            let start = into.len();
            while into.len().saturating_sub(start) < limit {
                match self.from_reader.try_recv() {
                    Ok(FromChild::Bytes(chunk)) => into.extend_from_slice(&chunk),
                    Ok(FromChild::Closed) | Err(TryRecvError::Disconnected) => {
                        self.output_closed = true;
                        return;
                    }
                    Err(TryRecvError::Empty) => return,
                }
            }
        }

        fn resize(&mut self, size: WinSize) {
            // A failure means the child is gone or the descriptor is not a
            // terminal; the grid has resized either way, and the exit, if that
            // is what it was, is reported by `poll_exit`.
            let _ = libcall::pty::set_window_size(self.master.as_raw_fd(), size);
        }

        fn poll_exit(&mut self) -> Option<Exit> {
            if self.reported {
                return None;
            }
            if self.reaped.is_none() {
                let exit = match libcall::pty::try_wait(self.pid) {
                    Ok(ChildState::Running) => return None,
                    Ok(ChildState::Exited(code)) => Exit::Code(code),
                    Ok(ChildState::Signaled(sig)) => Exit::Signal(sig),
                    Err(errno) => Exit::Lost(errno),
                };
                self.reaped = Some((exit, Instant::now()));
            }
            let (exit, when) = self.reaped?;
            if self.output_closed || when.elapsed() >= OUTPUT_GRACE {
                self.reported = true;
                Some(exit)
            } else {
                None
            }
        }

        fn set_waker(&mut self, waker: Waker) -> bool {
            // Woken at once as well: whatever the child wrote before there was
            // a waker to wake is waiting, and would otherwise wait for the
            // child's next word.
            waker.wake_by_ref();
            *self
                .wake
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(waker);
            true
        }

        fn hang_up(&mut self) {
            if self.hung_up {
                return;
            }
            self.hung_up = true;
            // Closing the writer's queue lets that thread finish.
            self.to_writer = None;
            if self.reaped.is_none() {
                // `SIGHUP` is what a terminal going away means to a shell,
                // which passes it on to its jobs. It is sent rather than left
                // to the kernel's hang-up on the last close of the master,
                // because the reader thread holds a duplicate of the master
                // until the child closes the slave -- which it will not do
                // until it is told. A child that has already exited makes this
                // fail with ESRCH, which is the outcome wanted.
                let _ = libcall::kill(self.pid, libcall::pty::SIGHUP);
            }
        }
    }

    impl Drop for PtyLink {
        fn drop(&mut self) {
            self.hang_up();
            // From here the waiter collects the child; if it already found it
            // finished, nobody will again, so it is collected here -- but only
            // if `poll_exit` has not, because a second collection could take
            // some other child that has since been given the same id.
            let mut state = self
                .collect
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.link_gone = true;
            if state.finished && self.reaped.is_none() {
                let _ = libcall::pty::try_wait(self.pid);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        #![allow(
            clippy::unwrap_used,
            clippy::expect_used,
            clippy::panic,
            clippy::arithmetic_side_effects
        )]

        use super::*;

        const SH: &CStr = c"/bin/sh";

        fn size(rows: u16, cols: u16) -> WinSize {
            WinSize {
                rows,
                cols,
                xpixel: 0,
                ypixel: 0,
            }
        }

        /// Receive until `want` appears in the output or two seconds pass.
        fn receive_until(link: &mut PtyLink, want: &str) -> String {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut got = Vec::new();
            loop {
                link.receive(&mut got, usize::MAX);
                // For the assertion message only.
                let text = String::from_utf8_lossy(&got).into_owned();
                if text.contains(want) {
                    return text;
                }
                assert!(
                    Instant::now() < deadline,
                    "never saw {want:?}; got {text:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn wait_exit(link: &mut PtyLink) -> Exit {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let mut sink = Vec::new();
                link.receive(&mut sink, usize::MAX);
                if let Some(exit) = link.poll_exit() {
                    return exit;
                }
                assert!(Instant::now() < deadline, "the child never exited");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        /// A line typed at the master is run by a shell on the slave, and its
        /// output comes back: the round trip the whole module exists for.
        #[test]
        fn a_command_typed_at_the_link_runs_and_answers() {
            let mut link = PtyLink::spawn(SH, &[c"sh"], &[c"PS1=$ "], size(24, 80)).unwrap();
            // Carriage return, as the emulator sends for Enter: the line
            // discipline's ICRNL turns it into the newline the shell reads.
            // The arithmetic is there so that the answer cannot be the echo:
            // the echo says `$((20+1))`, and only the shell says `21`.
            let line = b"echo round-$((20+1))-trip\r";
            assert_eq!(link.send(line), line.len());
            receive_until(&mut link, "round-21-trip");
            link.send(b"exit 5\r");
            assert_eq!(wait_exit(&mut link), Exit::Code(5));
            assert_eq!(link.poll_exit(), None, "the exit was reported twice");
        }

        /// The shell sees the size it was given, and a resize reaches it.
        #[test]
        fn the_shell_sees_the_window_size_and_its_changes() {
            let mut link = PtyLink::spawn(SH, &[c"sh"], &[], size(30, 90)).unwrap();
            link.send(b"stty size\r");
            receive_until(&mut link, "30 90");
            link.resize(size(45, 120));
            link.send(b"stty size\r");
            receive_until(&mut link, "45 120");
            link.send(b"exit\r");
            assert_eq!(wait_exit(&mut link), Exit::Code(0));
        }

        /// Hanging up ends the shell, as closing the window must.
        #[test]
        fn hanging_up_ends_the_shell() {
            let mut link = PtyLink::spawn(SH, &[c"sh"], &[], size(24, 80)).unwrap();
            link.send(b"echo up\r");
            receive_until(&mut link, "up");
            link.hang_up();
            match wait_exit(&mut link) {
                Exit::Signal(1) | Exit::Code(129) => {}
                other => panic!("a hung-up shell {other}"),
            }
        }

        /// A waker given to the link is woken for output and for the exit,
        /// with nobody asking in between -- including a shell that exits
        /// while a job it started still holds the terminal open.
        #[test]
        fn the_link_wakes_for_output_and_for_an_exit_nothing_else_announces() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            use std::task::Wake;

            struct Count(AtomicUsize);
            impl Wake for Count {
                fn wake(self: Arc<Self>) {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
            let count = Arc::new(Count(AtomicUsize::new(0)));
            let woken = |c: &Arc<Count>| c.0.load(Ordering::SeqCst);

            let mut link = PtyLink::spawn(
                SH,
                &[c"sh", c"-c", c"sleep 0.2; echo hello; sleep 30 & exit 4"],
                &[],
                size(24, 80),
            )
            .unwrap();
            assert!(link.set_waker(Waker::from(Arc::clone(&count))));
            let at_start = woken(&count);
            let deadline = Instant::now() + Duration::from_secs(3);
            while woken(&count) == at_start {
                assert!(Instant::now() < deadline, "never woken for the output");
                std::thread::sleep(Duration::from_millis(5));
            }
            receive_until(&mut link, "hello");
            // The background `sleep` holds the slave, so the output never
            // closes: only the waiter can say the shell has gone.
            let deadline = Instant::now() + Duration::from_secs(3);
            let exit = loop {
                let before = woken(&count);
                if let Some(exit) = link.poll_exit() {
                    break exit;
                }
                while woken(&count) == before {
                    assert!(Instant::now() < deadline, "the exit woke nobody");
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            assert_eq!(exit, Exit::Code(4));
        }

        /// The exit is announced after the child's last output, not before.
        #[test]
        fn the_last_output_arrives_before_the_exit() {
            let mut link = PtyLink::spawn(
                SH,
                &[c"sh", c"-c", c"printf 'last words'; exit 3"],
                &[],
                size(24, 80),
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut got = Vec::new();
            loop {
                link.receive(&mut got, usize::MAX);
                if let Some(exit) = link.poll_exit() {
                    assert_eq!(exit, Exit::Code(3));
                    break;
                }
                assert!(Instant::now() < deadline, "the child never exited");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                String::from_utf8_lossy(&got).contains("last words"),
                "the exit was announced before the output it followed"
            );
        }
    }
}

/// A child made of a script, for the emulator's own tests.
///
/// The emulator's tests are about the emulator: what it sends for a key, what
/// it draws for a byte, when it says the child has gone. A shell would make
/// every one of them depend on a process, a clock and a platform; this makes
/// them depend on nothing, and the real link has its own tests above.
///
/// Behind the `testing` feature, which the applications turn on in their
/// `[dev-dependencies]`, so their tests can attach one.
#[cfg(any(test, feature = "testing"))]
pub mod script {
    use super::{Exit, Link};
    use libcall::pty::WinSize;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What the scripted child has been sent, and what it will say.
    #[derive(Debug, Default)]
    pub struct Script {
        /// Bytes the child will "write", handed over on the next `receive`.
        pub pending: Vec<u8>,
        /// Everything the terminal sent, in order.
        pub sent: Vec<u8>,
        /// The most the child will ever have taken, or `None` for no limit --
        /// a child that has stopped reading.
        pub capacity: Option<usize>,
        /// The last size the terminal reported.
        pub size: Option<WinSize>,
        /// An exit to report on the next `poll_exit`.
        pub exit: Option<Exit>,
        /// How many times the terminal hung up.
        pub hang_ups: usize,
        /// The waker the terminal handed over, if it did.
        pub waker: Option<std::task::Waker>,
        /// Whether this link says it will wake the terminal. A test that sets
        /// it wakes the terminal itself, through `waker`.
        pub wakes: bool,
    }

    /// A [`Link`] over a shared [`Script`], which the test keeps a handle to.
    pub struct ScriptLink(pub Rc<RefCell<Script>>);

    impl ScriptLink {
        /// A link and the handle to drive it with.
        #[must_use]
        pub fn new() -> (Self, Rc<RefCell<Script>>) {
            let script = Rc::new(RefCell::new(Script::default()));
            (Self(Rc::clone(&script)), script)
        }
    }

    impl Link for ScriptLink {
        fn send(&mut self, bytes: &[u8]) -> usize {
            let mut s = self.0.borrow_mut();
            let room = s
                .capacity
                .map_or(bytes.len(), |cap| cap.saturating_sub(s.sent.len()));
            let n = room.min(bytes.len());
            s.sent.extend_from_slice(bytes.get(..n).unwrap_or_default());
            n
        }

        fn receive(&mut self, into: &mut Vec<u8>, limit: usize) {
            let mut s = self.0.borrow_mut();
            let n = limit.min(s.pending.len());
            into.extend(s.pending.drain(..n));
        }

        fn resize(&mut self, size: WinSize) {
            self.0.borrow_mut().size = Some(size);
        }

        fn poll_exit(&mut self) -> Option<Exit> {
            self.0.borrow_mut().exit.take()
        }

        fn hang_up(&mut self) {
            let mut s = self.0.borrow_mut();
            s.hang_ups = s.hang_ups.saturating_add(1);
        }

        fn set_waker(&mut self, waker: std::task::Waker) -> bool {
            let mut s = self.0.borrow_mut();
            s.waker = Some(waker);
            s.wakes
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_relative_or_missing_shell_falls_back_to_bin_sh() {
        assert_eq!(shell_path(None), PathBuf::from("/bin/sh"));
        assert_eq!(shell_path(Some("zsh".into())), PathBuf::from("/bin/sh"));
        assert_eq!(shell_path(Some("".into())), PathBuf::from("/bin/sh"));
        assert_eq!(
            shell_path(Some("/usr/bin/fish".into())),
            PathBuf::from("/usr/bin/fish")
        );
    }

    #[test]
    fn the_shell_is_told_what_terminal_it_is_on_and_not_a_stale_size() {
        let env = shell_environment([
            (b"HOME".to_vec(), b"/home/u".to_vec()),
            (b"TERM".to_vec(), b"dumb".to_vec()),
            (b"COLUMNS".to_vec(), b"80".to_vec()),
            (b"LINES".to_vec(), b"24".to_vec()),
        ]);
        assert!(env.contains(&b"HOME=/home/u".to_vec()), "{env:?}");
        assert!(env.contains(&b"TERM=xterm-256color".to_vec()));
        assert!(env.contains(&b"COLORTERM=truecolor".to_vec()));
        assert!(!env.iter().any(|e| e.starts_with(b"TERM=dumb")));
        assert!(!env.iter().any(|e| e.starts_with(b"COLUMNS=")));
        assert!(!env.iter().any(|e| e.starts_with(b"LINES=")));
        assert_eq!(
            env.iter().filter(|e| e.starts_with(b"TERM=")).count(),
            1,
            "two TERMs: {env:?}"
        );
    }

    #[test]
    fn environment_bytes_pass_through_untouched() {
        // Not UTF-8, and must not be made so.
        let env = shell_environment([(b"NAME".to_vec(), vec![0xff, 0xfe, b'x'])]);
        assert!(env.contains(&b"NAME=\xff\xfex".to_vec()));
    }

    #[test]
    fn entries_that_cannot_cross_the_c_boundary_are_dropped_not_mangled() {
        let env = shell_environment([
            (b"A=B".to_vec(), b"v".to_vec()),
            (b"NUL".to_vec(), b"a\0b".to_vec()),
            (b"OK".to_vec(), b"fine".to_vec()),
        ]);
        assert!(env.contains(&b"OK=fine".to_vec()));
        assert!(!env.iter().any(|e| e.starts_with(b"A=B")));
        assert!(!env.iter().any(|e| e.starts_with(b"NUL=")));
    }

    #[test]
    fn a_spawn_failure_says_what_and_why() {
        let err = SpawnError {
            program: PathBuf::from("/bin/zsh"),
            errno: libcall::ENOENT,
        };
        let text = err.to_string();
        assert!(text.contains("/bin/zsh"), "{text}");
        assert!(text.contains("no such file"), "{text}");

        let none = SpawnError {
            program: PathBuf::from("/bin/sh"),
            errno: libcall::ENOSYS,
        };
        assert!(none.to_string().contains("no pseudo-terminals"), "{none}");
    }

    #[test]
    fn an_exit_says_how_it_ended() {
        assert_eq!(Exit::Code(3).to_string(), "exited with status 3");
        assert!(Exit::Signal(9).to_string().contains("SIGKILL"));
        assert!(Exit::Signal(64).to_string().contains("signal 64"));
        assert!(
            Exit::Lost(libcall::pty::ECHILD)
                .to_string()
                .contains("could not be collected")
        );
        assert!(Exit::Code(0).is_clean());
        assert!(!Exit::Code(1).is_clean());
        assert!(!Exit::Signal(15).is_clean());
    }

    #[test]
    fn an_unknown_errno_is_named_by_number_not_given_a_meaning() {
        assert_eq!(describe_errno(9999), "error 9999");
    }

    /// On a host with no pseudo-terminals the answer is "none here", from
    /// the library rather than invented by the terminal.
    #[cfg(not(unix))]
    #[test]
    fn a_host_without_terminals_says_so() {
        let err = spawn_shell(WinSize::default())
            .err()
            .expect("no shell on a host");
        assert_eq!(err.errno, libcall::ENOSYS);
    }
}
