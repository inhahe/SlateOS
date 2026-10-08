//! `syslogd daemon`: the system log's socket, read into the journal.
//!
//! Every program that logs the POSIX way -- the C library's `syslog()`,
//! `logger` -- sends each message as one datagram to `/dev/log`. This binds
//! that socket as systemd-journald binds it on Linux, a datagram socket
//! anyone may write to, and appends each datagram to the journal `journalctl`
//! reads, as [`crate::record::line`] spells it, with the sender's process,
//! user and group as the kernel vouches for them.
//!
//! Once it is listening, nothing that goes wrong stops it: a datagram it
//! cannot receive or a record it cannot write is reported -- at most once a
//! minute for each kind of failure, with how many more were held back -- and
//! the next one is tried. A logger that died on a full disk would take the
//! report of the full disk with it.

use crate::record;
use crate::sys;
use quoting::quotef_os;
use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where the daemon listens, and where it files what it hears.
pub struct Options {
    /// `/dev/log` unless `--socket` says otherwise.
    pub socket: PathBuf,
    /// The journal file, `/var/log/syslog.jsonl` unless `--journal` says
    /// otherwise.
    pub journal: PathBuf,
}

/// `syslogd: MESSAGE` on standard error. A daemon's standard error may go
/// nowhere at all, and a report that cannot be made has nowhere else to go.
fn say(message: &str) {
    // Nothing to be done about a lost report but to carry on.
    let _ = writeln!(io::stderr().lock(), "syslogd: {message}");
}

/// One kind of failure, said at most once a minute.
#[derive(Default)]
struct Complaint {
    last: Option<Instant>,
    held: u64,
}

impl Complaint {
    fn about(&mut self, message: &str) {
        let now = Instant::now();
        if self
            .last
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(60))
        {
            self.held = self.held.saturating_add(1);
            return;
        }
        if self.held > 0 {
            say(&format!("{message} ({} more not shown)", self.held));
        } else {
            say(message);
        }
        self.last = Some(now);
        self.held = 0;
    }
}

fn strerror(e: &io::Error) -> String {
    errmsg::strerror(e)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The command name of process `pid`, as `/proc/PID/comm` gives it --
/// journald's `_COMM`. `None` when the process has already gone.
fn command_name(pid: i32) -> Option<Vec<u8>> {
    let mut comm = fs::read(format!("/proc/{pid}/comm")).ok()?;
    if comm.last() == Some(&b'\n') {
        comm.pop();
    }
    Some(comm)
}

/// Make the socket at `path`. A socket a previous daemon left there is
/// replaced; anything else at the path is not this daemon's to remove.
fn bind(path: &Path) -> Result<UnixDatagram, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path).map_err(|e| {
            format!(
                "cannot remove the old socket {}: {}",
                quotef_os(path.as_os_str()),
                strerror(&e)
            )
        })?,
        Ok(_) => {
            return Err(format!(
                "{} exists and is not a socket",
                quotef_os(path.as_os_str())
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "cannot look at {}: {}",
                quotef_os(path.as_os_str()),
                strerror(&e)
            ));
        }
    }
    let socket = UnixDatagram::bind(path).map_err(|e| {
        format!(
            "cannot listen on {}: {}",
            quotef_os(path.as_os_str()),
            strerror(&e)
        )
    })?;
    // Anyone may log, as anyone may write to journald's `/dev/log`.
    fs::set_permissions(path, fs::Permissions::from_mode(0o666)).map_err(|e| {
        format!(
            "cannot open {} to every user: {}",
            quotef_os(path.as_os_str()),
            strerror(&e)
        )
    })?;
    sys::pass_credentials(socket.as_raw_fd())
        .map_err(|e| format!("cannot ask for senders' credentials: {}", strerror(&e)))?;
    Ok(socket)
}

/// Listen until killed: each datagram received becomes one journal record.
/// Returns only when the socket cannot be made, with status 1.
///
/// `rotate` moves the journal aside once it grows past `max_size`, and
/// returns each step of that which failed; the first is reported, as any
/// other failure here is, and the rotation is tried again by the next record.
pub fn run(
    opts: &Options,
    max_size: u64,
    rotate: &dyn Fn(&Path) -> Vec<(PathBuf, io::Error)>,
) -> ExitCode {
    let socket = match bind(&opts.socket) {
        Ok(s) => s,
        Err(message) => {
            say(&message);
            return ExitCode::FAILURE;
        }
    };
    let mut buf = vec![0u8; sys::MAX_DATAGRAM];
    let mut receiving = Complaint::default();
    let mut writing = Complaint::default();
    let mut cut = Complaint::default();
    let mut rotating = Complaint::default();
    loop {
        let (len, truncated, creds) = match sys::receive(socket.as_raw_fd(), &mut buf) {
            Ok(got) => got,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                receiving.about(&format!("cannot receive a message: {}", strerror(&e)));
                // A failure that repeats at once would otherwise spin.
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        // journald files nothing for an empty datagram.
        if len == 0 {
            continue;
        }
        if truncated {
            cut.about("a message longer than 256 KiB was cut short");
        }
        let raw = buf.get(..len).unwrap_or_default();
        let comm = creds.and_then(|c| command_name(c.pid));
        let mut line = record::line(raw, now_secs(), creds, comm.as_deref()).into_bytes();
        line.push(b'\n');
        if let Err(e) = journalio::append(&opts.journal, &line) {
            writing.about(&format!(
                "cannot write to {}: {}",
                quotef_os(opts.journal.as_os_str()),
                strerror(&e)
            ));
            continue;
        }
        if fs::metadata(&opts.journal).is_ok_and(|m| m.len() > max_size)
            && let Some((path, e)) = rotate(&opts.journal).first()
        {
            rotating.about(&format!(
                "cannot rotate {}: {}",
                quotef_os(path.as_os_str()),
                strerror(e)
            ));
        }
    }
}
