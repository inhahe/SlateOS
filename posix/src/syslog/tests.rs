//! glibc 2.39's answers, replayed; and the journal, which has no oracle.
//!
//! `syslog_oracle.txt` is what `posix/tools/oracle/syslog_harness.py` saw
//! glibc's logger do under WSL, scenario by scenario, each a fresh process:
//! every record its daemon at `/dev/log` received, what `LOG_PERROR` wrote to
//! standard error and `LOG_CONS` to the console, and what `setlogmask`
//! returned. [`every_scenario_is_glibcs`] reads the statements out of it, runs
//! them here against [`world`] -- a system these tests own, with a daemon they
//! start, stop and restart -- and compares this library's transcript with
//! glibc's, line for line.
//!
//! The journal has no oracle, glibc having no journal, so its tests say what
//! it must do: write the record `journalrec` spells (a dev-dependency, so that
//! the copy of its escaper in `syslog.rs` cannot drift from it), follow
//! `journalio`'s protocol, and sit inside glibc's retries like any connection.

use std::format;
use std::string::{String, ToString};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::vec::Vec;

use self::world::World;
use super::*;

// ---------------------------------------------------------------------------
// The world
// ---------------------------------------------------------------------------

/// The test build's system: what `syslog.rs`'s `sys` calls reach.
///
/// A daemon at `/dev/log` that the tests start, stop and restart, as the
/// oracle's harness does to glibc; a console; standard error; a journal file
/// system with its lock, which the tests can make fail or replace; a clock and
/// a process. Every call is recorded in `trace`, so a test can say not only
/// what was written but what was done to write it.
pub(super) mod world {
    use std::collections::BTreeMap;
    use std::format;
    use std::string::String;
    use std::sync::{Mutex, PoisonError};
    use std::vec::Vec;

    use crate::errno;
    use crate::fcntl::O_CREAT;
    use crate::socket::{AF_UNIX, SOCK_CLOEXEC, SOCK_STREAM};
    use crate::time::Tm;

    /// What `/dev/log` names.
    #[derive(Clone, Copy, Debug)]
    pub struct Daemon {
        /// Which daemon: each start is a new one, at a new socket.
        pub generation: u32,
        /// `SOCK_DGRAM` or `SOCK_STREAM`.
        pub kind: i32,
        /// Whether anything listens on its socket.
        pub listening: bool,
    }

    #[derive(Clone, Copy, Debug)]
    enum Open {
        /// A socket, and the daemon and connection it reached.
        Socket { kind: i32, peer: Option<(u32, u64)> },
        Console,
        File { ino: u64 },
    }

    /// What reached the daemon: a datagram, or what one stream connection
    /// carried since the last look.
    #[derive(Clone, Debug)]
    pub struct Delivery {
        pub connection: Option<u64>,
        pub bytes: Vec<u8>,
    }

    #[derive(Debug)]
    pub struct World {
        /// Whether `AF_UNIX` sockets exist; when not, `socket` is
        /// `EAFNOSUPPORT`, as on SlateOS today.
        pub unix: bool,
        pub daemon: Option<Daemon>,
        generations: u32,
        connections: u64,
        fds: BTreeMap<i32, Open>,
        next_fd: i32,
        pub deliveries: Vec<Delivery>,
        pub stderr: Vec<u8>,
        pub console: Vec<u8>,
        /// Whether `/dev/console` opens.
        pub console_opens: bool,
        pub pid: i32,
        pub now: i64,
        /// What `localtime_r` gives: year-1900, month, day, hour, minute,
        /// second; `None` when it fails.
        pub clock: Option<[i32; 6]>,
        /// `__progname`, NUL-terminated; `None` for NULL.
        pub progname: Option<&'static [u8]>,
        /// The file system: path to inode, inode to contents.
        pub paths: BTreeMap<Vec<u8>, u64>,
        pub inodes: BTreeMap<u64, Vec<u8>>,
        next_ino: u64,
        /// What the next `socket`s fail with, first first, where sockets
        /// exist.
        pub socket_errors: Vec<i32>,
        /// What the next opens of the journal fail with.
        pub open_errors: Vec<i32>,
        /// What the next `flock`s fail with.
        pub flock_errors: Vec<i32>,
        /// What the next `stat`s of the journal fail with.
        pub stat_errors: Vec<i32>,
        /// What the next writes to the journal fail with.
        pub write_errors: Vec<i32>,
        /// At most this many bytes per write to the journal.
        pub write_limit: Option<usize>,
        /// How many of the next locks find the journal renamed away -- a
        /// rotation while the lock was awaited.
        pub rotations: u32,
        /// How many find it renamed over by a rewrite holding `rewrite_with`.
        pub rewrites: u32,
        pub rewrite_with: Vec<u8>,
        /// Whether `malloc` fails.
        pub alloc_fails: bool,
        pub allocations: usize,
        /// Every call, in order.
        pub trace: Vec<String>,
    }

    impl World {
        pub const fn new() -> Self {
            Self {
                unix: true,
                daemon: None,
                generations: 0,
                connections: 0,
                fds: BTreeMap::new(),
                next_fd: 3,
                deliveries: Vec::new(),
                stderr: Vec::new(),
                console: Vec::new(),
                console_opens: true,
                pid: 4242,
                now: 1_790_843_180,
                clock: Some([126, 9, 1, 8, 26, 20]),
                progname: Some(b"syslog-oracle\0"),
                paths: BTreeMap::new(),
                inodes: BTreeMap::new(),
                next_ino: 1,
                socket_errors: Vec::new(),
                open_errors: Vec::new(),
                flock_errors: Vec::new(),
                stat_errors: Vec::new(),
                write_errors: Vec::new(),
                write_limit: None,
                rotations: 0,
                rewrites: 0,
                rewrite_with: Vec::new(),
                alloc_fails: false,
                allocations: 0,
                trace: Vec::new(),
            }
        }

        /// A daemon of `kind` listening at a new socket, which `/dev/log` now
        /// names.
        pub fn start(&mut self, kind: i32) {
            self.generations += 1;
            self.daemon = Some(Daemon {
                generation: self.generations,
                kind,
                listening: true,
            });
        }

        /// The daemon gone: its socket closed, the path left naming it.
        pub fn stop(&mut self) {
            if let Some(d) = &mut self.daemon {
                d.listening = false;
            }
        }

        /// The journal's contents, when there is one.
        pub fn journal(&self) -> Option<&[u8]> {
            let ino = self.paths.get(super::super::JOURNAL_PATH.strip_suffix(b"\0")?)?;
            self.inodes.get(ino).map(Vec::as_slice)
        }

        /// Whether a descriptor is open: none should be, between calls.
        pub fn open_descriptors(&self) -> usize {
            self.fds.len()
        }

        fn add(&mut self, open: Open) -> i32 {
            let fd = self.next_fd;
            self.next_fd += 1;
            self.fds.insert(fd, open);
            fd
        }

        fn new_inode(&mut self, contents: Vec<u8>) -> u64 {
            let ino = self.next_ino;
            self.next_ino += 1;
            self.inodes.insert(ino, contents);
            ino
        }

        fn write(&mut self, fd: i32, data: &[u8]) -> isize {
            if fd == 2 {
                self.stderr.extend_from_slice(data);
                return data.len() as isize;
            }
            match self.fds.get(&fd).copied() {
                Some(Open::Console) => {
                    self.console.extend_from_slice(data);
                    data.len() as isize
                }
                Some(Open::File { ino }) => {
                    if !self.write_errors.is_empty() {
                        let e = self.write_errors.remove(0);
                        return fail(e, -1);
                    }
                    let n = self.write_limit.map_or(data.len(), |l| l.min(data.len()));
                    self.inodes.entry(ino).or_default().extend_from_slice(&data[..n]);
                    n as isize
                }
                _ => fail(errno::EBADF, -1),
            }
        }
    }

    pub static WORLD: Mutex<World> = Mutex::new(World::new());

    /// `f` with the world.
    pub fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        let mut w = WORLD.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut w)
    }

    fn fail<T>(e: i32, v: T) -> T {
        errno::set_errno(e);
        v
    }

    fn name(path: &[u8]) -> &[u8] {
        path.strip_suffix(b"\0").expect("a path the library passes is NUL-terminated")
    }

    // -- What `syslog.rs`'s `sys` provides ---------------------------------

    pub(in crate::syslog) fn socket(domain: i32, ty: i32, _protocol: i32) -> i32 {
        with(|w| {
            w.trace.push(format!("socket {}", ty & !SOCK_CLOEXEC));
            assert_ne!(ty & SOCK_CLOEXEC, 0, "glibc's socket is close-on-exec");
            if domain != AF_UNIX || !w.unix {
                return fail(errno::EAFNOSUPPORT, -1);
            }
            if !w.socket_errors.is_empty() {
                let e = w.socket_errors.remove(0);
                return fail(e, -1);
            }
            w.add(Open::Socket {
                kind: ty & !SOCK_CLOEXEC,
                peer: None,
            })
        })
    }

    pub(in crate::syslog) fn connect(fd: i32, path: &[u8]) -> i32 {
        with(|w| {
            w.trace.push(format!("connect {fd}"));
            assert_eq!(name(path), b"/dev/log", "where the daemon is");
            let Some(Open::Socket { kind, .. }) = w.fds.get(&fd).copied() else {
                return fail(errno::EBADF, -1);
            };
            let d = match w.daemon {
                None => return fail(errno::ENOENT, -1),
                Some(d) if !d.listening => return fail(errno::ECONNREFUSED, -1),
                Some(d) if d.kind != kind => return fail(errno::EPROTOTYPE, -1),
                Some(d) => d,
            };
            w.connections += 1;
            let connection = w.connections;
            w.fds.insert(
                fd,
                Open::Socket {
                    kind,
                    peer: Some((d.generation, connection)),
                },
            );
            0
        })
    }

    pub(in crate::syslog) fn send(fd: i32, data: &[u8]) -> isize {
        with(|w| {
            w.trace.push(format!("send {fd} {}", data.len()));
            let Some(Open::Socket { kind, peer }) = w.fds.get(&fd).copied() else {
                return fail(errno::EBADF, -1);
            };
            let alive = matches!((w.daemon, peer),
                (Some(d), Some((g, _))) if d.listening && d.generation == g);
            if !alive {
                // What Linux says on a connection whose daemon has gone.
                let e = if kind == SOCK_STREAM {
                    errno::EPIPE
                } else {
                    errno::ECONNREFUSED
                };
                return fail(e, -1);
            }
            let connection = if kind == SOCK_STREAM {
                peer.map(|(_, c)| c)
            } else {
                None
            };
            match w.deliveries.last_mut() {
                Some(last) if connection.is_some() && last.connection == connection => {
                    last.bytes.extend_from_slice(data);
                }
                _ => w.deliveries.push(Delivery {
                    connection,
                    bytes: data.to_vec(),
                }),
            }
            data.len() as isize
        })
    }

    pub(in crate::syslog) fn open(path: &[u8], flags: i32, mode: u32) -> i32 {
        with(|w| {
            let path = name(path);
            w.trace.push(format!(
                "open {} {flags:#o} {mode:#o}",
                std::str::from_utf8(path).expect("an ASCII path")
            ));
            if path == b"/dev/console" {
                return if w.console_opens {
                    w.add(Open::Console)
                } else {
                    fail(errno::ENOENT, -1)
                };
            }
            if !w.open_errors.is_empty() {
                let e = w.open_errors.remove(0);
                return fail(e, -1);
            }
            let ino = match w.paths.get(path).copied() {
                Some(ino) => ino,
                None if flags & O_CREAT != 0 => {
                    let ino = w.new_inode(Vec::new());
                    w.paths.insert(path.to_vec(), ino);
                    ino
                }
                None => return fail(errno::ENOENT, -1),
            };
            w.add(Open::File { ino })
        })
    }

    pub(in crate::syslog) fn write(fd: i32, data: &[u8]) -> isize {
        with(|w| {
            w.trace.push(format!("write {fd} {}", data.len()));
            w.write(fd, data)
        })
    }

    pub(in crate::syslog) fn writev2(fd: i32, a: &[u8], b: &[u8]) -> isize {
        with(|w| {
            w.trace.push(format!("writev {fd} {}", a.len() + b.len()));
            w.write(fd, &[a, b].concat())
        })
    }

    pub(in crate::syslog) fn close(fd: i32) {
        with(|w| {
            w.trace.push(format!("close {fd}"));
            assert!(
                w.fds.remove(&fd).is_some(),
                "closed a descriptor that is not open: {fd}"
            );
        });
    }

    pub(in crate::syslog) fn flock(fd: i32, op: i32) -> i32 {
        with(|w| {
            w.trace.push(format!("flock {fd} {op}"));
            if !w.flock_errors.is_empty() {
                let e = w.flock_errors.remove(0);
                return fail(e, -1);
            }
            let journal = name(super::super::JOURNAL_PATH).to_vec();
            if w.rotations > 0 {
                w.rotations -= 1;
                w.paths.remove(&journal);
            } else if w.rewrites > 0 {
                w.rewrites -= 1;
                let contents = w.rewrite_with.clone();
                let ino = w.new_inode(contents);
                w.paths.insert(journal, ino);
            }
            0
        })
    }

    pub(in crate::syslog) fn same_file(fd: i32, path: &[u8]) -> Result<bool, i32> {
        with(|w| {
            w.trace.push(format!("same {fd}"));
            let Some(Open::File { ino }) = w.fds.get(&fd).copied() else {
                return Err(errno::EBADF);
            };
            if !w.stat_errors.is_empty() {
                return Err(w.stat_errors.remove(0));
            }
            match w.paths.get(name(path)) {
                None => Err(errno::ENOENT),
                Some(&now) => Ok(now == ino),
            }
        })
    }

    pub(in crate::syslog) fn getpid() -> i32 {
        with(|w| w.pid)
    }

    pub(in crate::syslog) fn now() -> i64 {
        with(|w| w.now)
    }

    pub(in crate::syslog) fn local_time(_t: i64) -> Option<Tm> {
        with(|w| {
            w.clock.map(|[year, mon, mday, hour, min, sec]| Tm {
                tm_year: year,
                tm_mon: mon,
                tm_mday: mday,
                tm_hour: hour,
                tm_min: min,
                tm_sec: sec,
                ..Tm::ZERO
            })
        })
    }

    pub(in crate::syslog) fn progname() -> *const u8 {
        with(|w| w.progname.map_or(core::ptr::null(), <[u8]>::as_ptr))
    }

    pub(in crate::syslog) fn alloc(len: usize) -> *mut u8 {
        with(|w| {
            w.allocations += 1;
            if w.alloc_fails {
                core::ptr::null_mut()
            } else {
                crate::malloc::malloc(len)
            }
        })
    }

    /// # Safety
    ///
    /// `ptr` came from [`alloc`].
    pub(in crate::syslog) unsafe fn free(ptr: *mut u8) {
        // SAFETY: the caller's contract.
        unsafe { crate::malloc::free(ptr) };
    }
}

/// Serialises the tests: the logger and the world are the process's.
static SERIAL: Mutex<()> = Mutex::new(());

/// A fresh process: the logger as glibc starts one, and `w`.
#[must_use = "the guard keeps the other tests out; bind it to `_g`"]
fn fresh(w: World) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    *Locked::take().state() = State::FRESH;
    world::with(|old| *old = w);
    g
}

/// The world glibc's scenarios began in: Unix sockets, a daemon of `kind`
/// at `/dev/log` (`none`: a socket nobody listens on), a console that opens.
fn glibc_world(kind: &str) -> World {
    let mut w = World::new();
    match kind {
        "dgram" => w.start(SOCK_DGRAM),
        "stream" => w.start(SOCK_STREAM),
        "none" => {
            w.start(SOCK_DGRAM);
            w.stop();
        }
        other => panic!("no such daemon: {other}"),
    }
    w
}

/// SlateOS's world: no Unix-domain sockets, so the journal.
fn slate_world() -> World {
    let mut w = World::new();
    w.unix = false;
    w
}

/// The timestamp the world's clock reads as.
const STAMP: &[u8; 16] = b"Oct  1 08:26:20 ";

/// A `va_list` holding `gp` (integers and pointers) and `fp` (doubles), in
/// registers.
fn with_va<R>(gp: &[u64], fp: &[f64], f: impl FnOnce(*mut VaList) -> R) -> R {
    assert!(gp.len() <= 6 && fp.len() <= 8, "more than the registers hold");
    let mut reg = [0u8; 176];
    for (i, v) in gp.iter().enumerate() {
        reg[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in fp.iter().enumerate() {
        reg[48 + i * 16..48 + i * 16 + 8].copy_from_slice(&v.to_bits().to_le_bytes());
    }
    let mut overflow = [0u8; 16];
    let mut va = VaList {
        gp_offset: 0,
        fp_offset: 48,
        overflow_arg_area: overflow.as_mut_ptr(),
        reg_save_area: reg.as_mut_ptr(),
    };
    f(&mut va)
}

/// `syslog (pri, "%s", msg)`.
fn log(pri: i32, msg: &[u8]) {
    let mut text = msg.to_vec();
    text.push(0);
    with_va(&[text.as_ptr() as u64], &[], |va| {
        // SAFETY: "%s" reads the one pointer the va_list holds, to a
        // NUL-terminated string that outlives the call.
        unsafe { vsyslog(pri, b"%s\0".as_ptr(), va) };
    });
}

/// What the daemon, standard error and the console got since the last look.
fn look() -> (Vec<Vec<u8>>, Vec<u8>, Vec<u8>) {
    world::with(|w| {
        let sent = core::mem::take(&mut w.deliveries)
            .into_iter()
            .map(|d| d.bytes)
            .collect();
        (
            sent,
            core::mem::take(&mut w.stderr),
            core::mem::take(&mut w.console),
        )
    })
}

/// `<pri>` + the world's timestamp + `rest`.
fn record(pri: i32, rest: &[u8]) -> Vec<u8> {
    [format!("<{pri}>").as_bytes(), STAMP, rest].concat()
}

/// The journal line `journalrec` writes for these fields, newline included.
fn journalrec_line(level: &str, service: &str, msg: &str, pid: u32, facility: Option<&str>) -> Vec<u8> {
    let r = journalrec::Record {
        ts: 1_790_843_180,
        level: level.to_string(),
        service: service.to_string(),
        msg: msg.to_string(),
        pid: Some(pid),
    };
    let extra: Vec<(String, String)> = facility
        .map(|f| vec![("facility".to_string(), f.to_string())])
        .unwrap_or_default();
    let mut line = r.to_json_line_with(&extra).into_bytes();
    line.push(b'\n');
    line
}

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

static ORACLE: &str = include_str!("../syslog_oracle.txt");

/// Every scenario glibc ran, run here, its transcript the same line for line.
#[test]
fn every_scenario_is_glibcs() {
    let mut scenarios: Vec<Vec<&str>> = Vec::new();
    for line in ORACLE.lines() {
        if line.starts_with("=== ") {
            scenarios.push(Vec::new());
        }
        scenarios.last_mut().expect("the oracle starts with a scenario").push(line);
    }
    assert_eq!(scenarios.len(), 44, "scenarios in the oracle");
    let mut compared = 0;
    for theirs in &scenarios {
        let ours = replay(theirs);
        for (i, (a, b)) in ours.iter().zip(theirs.iter()).enumerate() {
            assert_eq!(
                a,
                b,
                "{}: line {} differs\nours:\n{}\nglibc's:\n{}",
                theirs[0],
                i + 1,
                ours.join("\n"),
                theirs.join("\n")
            );
        }
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{}: ours:\n{}\nglibc's:\n{}",
            theirs[0],
            ours.join("\n"),
            theirs.join("\n")
        );
        compared += theirs.len();
    }
    assert_eq!(compared, ORACLE.lines().count(), "every line compared");
}

/// One scenario, run here: its `===`, `daemon`, `decl` and `call` lines
/// echoed, and after each call what this library did, in the oracle's form.
fn replay(lines: &[&str]) -> Vec<String> {
    let kind = lines[1].strip_prefix("daemon ").expect("a daemon line");
    let _g = fresh(glibc_world(kind));
    let mut out: Vec<String> = lines[..2].iter().map(ToString::to_string).collect();
    // The buffers `decl` lines make, and the strings `openlog` was given:
    // the logger keeps their addresses, as glibc keeps them.
    let mut vars: Vec<(String, Vec<u8>)> = Vec::new();
    let mut kept: Vec<Vec<u8>> = Vec::new();
    for line in &lines[2..] {
        if let Some(decl) = line.strip_prefix("decl ") {
            vars.push(parse_decl(decl));
            out.push(line.to_string());
        } else if let Some(stmt) = line.strip_prefix("call ") {
            out.push(line.to_string());
            run(stmt, &vars, &mut kept, &mut out);
        }
    }
    let open = world::with(|w| w.open_descriptors());
    // The connection, if one is open, is the only descriptor left.
    assert!(open <= 1, "{}: {open} descriptors left open", lines[0]);
    out
}

/// One statement of a scenario.
fn run(stmt: &str, vars: &[(String, Vec<u8>)], kept: &mut Vec<Vec<u8>>, out: &mut Vec<String>) {
    if let Some(name) = stmt.strip_prefix("errno = ") {
        errno::set_errno(constant(name));
        return;
    }
    let (name, args) = split_call(stmt);
    match name {
        "openlog" => {
            let tag = match args[0] {
                "NULL" => core::ptr::null(),
                a if a.starts_with('"') => {
                    let mut s = c_string(a);
                    s.push(0);
                    kept.push(s);
                    kept.last().expect("just pushed").as_ptr()
                }
                var => variable(vars, var).as_ptr(),
            };
            openlog(tag, eval(args[1]), eval(args[2]));
        }
        "syslog" | "call_vsyslog" => {
            call_syslog(eval(args[0]), None, &args[1..], vars, kept);
            look_into(out);
        }
        "__syslog_chk" => {
            call_syslog(eval(args[0]), Some(eval(args[1])), &args[2..], vars, kept);
            look_into(out);
        }
        "setlogmask" => out.push(format!("returned {}", setlogmask(eval(args[0])))),
        "closelog" => closelog(),
        "daemon_stop" => world::with(World::stop),
        "daemon_start" => {
            let kind = eval(args[0]);
            world::with(|w| w.start(kind));
        }
        other => panic!("a statement the replay does not know: {other}"),
    }
}

/// `syslog`, `vsyslog` or `__syslog_chk` with the oracle's arguments: the
/// format, then integers, strings, buffers and doubles.
fn call_syslog(
    pri: i32,
    flag: Option<i32>,
    args: &[&str],
    vars: &[(String, Vec<u8>)],
    kept: &mut Vec<Vec<u8>>,
) {
    let mut fmt = c_string(args[0]);
    fmt.push(0);
    let (mut gp, mut fp) = (Vec::new(), Vec::new());
    for &a in &args[1..] {
        if a.starts_with('"') {
            let mut s = c_string(a);
            s.push(0);
            kept.push(s);
            gp.push(kept.last().expect("just pushed").as_ptr() as u64);
        } else if a.contains('.') {
            fp.push(a.parse::<f64>().expect("a double"));
        } else if a.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
            gp.push(i64::from(eval(a)) as u64);
        } else {
            gp.push(variable(vars, a).as_ptr() as u64);
        }
    }
    with_va(&gp, &fp, |va| match flag {
        // SAFETY: the va_list holds what the format converts, as the
        // oracle's C passed it.
        None => unsafe { vsyslog(pri, fmt.as_ptr(), va) },
        // SAFETY: as above.
        Some(flag) => unsafe { __vsyslog_chk(pri, flag, fmt.as_ptr(), va) },
    });
}

/// The oracle's lines for what the call did: each delivery, standard error,
/// the console -- timestamps checked and masked, PIDs masked.
fn look_into(out: &mut Vec<String>) {
    let (sent, stderr, console) = look();
    for bytes in sent {
        out.push(format!("sent {}", hex(&mask_pid(&mask_stamps(&bytes)))));
    }
    if !stderr.is_empty() {
        out.push(format!("stderr {}", hex(&mask_pid(&stderr))));
    }
    if !console.is_empty() {
        out.push(format!("console {}", hex(&mask_pid(&console))));
    }
}

/// The harness's `mask`: in each record -- a stream's end in a NUL each --
/// the sixteen bytes after the first `>` when they are a timestamp, which
/// must be [`STAMP`], as `@`s.
fn mask_stamps(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut start = 0;
    while start < out.len() {
        let end = out[start..]
            .iter()
            .position(|&b| b == 0)
            .map_or(out.len(), |p| start + p);
        let rec = &mut out[start..end];
        if let Some(gt) = rec.iter().position(|&b| b == b'>') {
            if gt + 17 <= rec.len() && rec[gt + 16] == b' ' && rec[gt + 4] == b' ' {
                assert_eq!(&rec[gt + 1..gt + 17], STAMP, "the record's timestamp");
                rec[gt + 1..gt + 17].fill(b'@');
            }
        }
        start = end + 1;
    }
    out
}

/// The harness's `mask_pids`: `[<pid>]` as `[PID]`.
fn mask_pid(bytes: &[u8]) -> Vec<u8> {
    let pid = format!("[{}]", world::with(|w| w.pid)).into_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(&pid) {
            out.extend_from_slice(b"[PID]");
            i += pid.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// The harness's `hex`: lowercase pairs, or `""` for nothing.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    if bytes.is_empty() {
        return "\"\"".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0xf)]));
    }
    out
}

/// `name(a, b, ...)` as the name and its top-level arguments.
fn split_call(stmt: &str) -> (&str, Vec<&str>) {
    let open = stmt.find('(').expect("a call");
    let inner = stmt[open + 1..].strip_suffix(')').expect("a call ends in )");
    let mut args = Vec::new();
    let (mut depth, mut quoted, mut escaped, mut start) = (0, false, false, 0);
    for (i, c) in inner.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !inner.trim().is_empty() {
        args.push(inner[start..].trim());
    }
    (&stmt[..open], args)
}

/// A C expression of the oracle's: `A | B | ...` over numbers, `<syslog.h>`
/// names, `LOG_UPTO (x)` and `LOG_MASK (x)`.
fn eval(expr: &str) -> i32 {
    let mut terms = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in expr.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '|' if depth == 0 => {
                terms.push(&expr[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    terms.push(&expr[start..]);
    terms.iter().fold(0, |acc, t| acc | term(t.trim()))
}

fn term(t: &str) -> i32 {
    if let Some(hex) = t.strip_prefix("0x") {
        return i32::from_str_radix(hex, 16).expect("hex");
    }
    if t.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        return t.parse().expect("a number");
    }
    if let Some(inner) = t.strip_prefix("LOG_UPTO(").and_then(|r| r.strip_suffix(')')) {
        return log_upto(eval(inner));
    }
    if let Some(inner) = t.strip_prefix("LOG_MASK(").and_then(|r| r.strip_suffix(')')) {
        return log_mask(eval(inner));
    }
    constant(t)
}

fn constant(name: &str) -> i32 {
    match name {
        "LOG_EMERG" => LOG_EMERG,
        "LOG_ALERT" => LOG_ALERT,
        "LOG_CRIT" => LOG_CRIT,
        "LOG_ERR" => LOG_ERR,
        "LOG_WARNING" => LOG_WARNING,
        "LOG_NOTICE" => LOG_NOTICE,
        "LOG_INFO" => LOG_INFO,
        "LOG_DEBUG" => LOG_DEBUG,
        "LOG_KERN" => LOG_KERN,
        "LOG_USER" => LOG_USER,
        "LOG_MAIL" => LOG_MAIL,
        "LOG_DAEMON" => LOG_DAEMON,
        "LOG_AUTH" => LOG_AUTH,
        "LOG_SYSLOG" => LOG_SYSLOG,
        "LOG_LPR" => LOG_LPR,
        "LOG_NEWS" => LOG_NEWS,
        "LOG_UUCP" => LOG_UUCP,
        "LOG_CRON" => LOG_CRON,
        "LOG_AUTHPRIV" => LOG_AUTHPRIV,
        "LOG_FTP" => LOG_FTP,
        "LOG_LOCAL0" => LOG_LOCAL0,
        "LOG_LOCAL3" => LOG_LOCAL3,
        "LOG_LOCAL7" => LOG_LOCAL7,
        "LOG_PID" => LOG_PID,
        "LOG_CONS" => LOG_CONS,
        "LOG_NDELAY" => LOG_NDELAY,
        "LOG_PERROR" => LOG_PERROR,
        "SOCK_DGRAM" => SOCK_DGRAM,
        "SOCK_STREAM" => SOCK_STREAM,
        "ENOENT" => errno::ENOENT,
        "EACCES" => errno::EACCES,
        other => panic!("a name the replay does not know: {other}"),
    }
}

/// A C string literal's bytes: `\n`, `\xHH`, `\\` and `\"` are what the
/// oracle's literals hold.
fn c_string(lit: &str) -> Vec<u8> {
    let body = lit
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .expect("a string literal");
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        match bytes[i + 1] {
            b'n' => {
                out.push(b'\n');
                i += 2;
            }
            b'x' => {
                let digits = &body[i + 2..i + 4];
                out.push(u8::from_str_radix(digits, 16).expect("\\x"));
                i += 4;
            }
            c => {
                out.push(c);
                i += 2;
            }
        }
    }
    out
}

/// `char NAME[N]; memset(NAME, 'C', sizeof NAME - 1); NAME[sizeof NAME - 1] = 0`
/// as NAME and its N bytes.
fn parse_decl(decl: &str) -> (String, Vec<u8>) {
    let rest = decl.strip_prefix("char ").expect("char");
    let (name, rest) = rest.split_once('[').expect("[");
    let (n, rest) = rest.split_once(']').expect("]");
    let fill = rest.split('\'').nth(1).expect("a fill character");
    let n: usize = n.parse().expect("a size");
    let mut buf = vec![fill.as_bytes()[0]; n - 1];
    buf.push(0);
    (name.to_string(), buf)
}

fn variable<'a>(vars: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &vars
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no buffer {name}"))
        .1
}

// ---------------------------------------------------------------------------
// glibc's paths the oracle cannot reach
// ---------------------------------------------------------------------------

/// The record that fills `bufs` exactly is built there; one byte more and it
/// is built on the heap. Either way it is whole.
#[test]
fn the_stack_buffer_and_the_heap_build_the_same_record() {
    // "<14>" + the timestamp + "syslog-oracle: " is 35 bytes of header.
    for (len, heap) in [(988, false), (989, true), (2000, true)] {
        let _g = fresh(glibc_world("dgram"));
        let msg = vec![b'm'; len];
        log(LOG_INFO, &msg);
        let (sent, _, _) = look();
        assert_eq!(sent, [record(14, &[b"syslog-oracle: ".as_slice(), &msg].concat())]);
        let allocations = world::with(|w| w.allocations);
        assert_eq!(allocations, usize::from(heap), "{len} bytes of message");
    }
}

/// With no memory for a long record, glibc sends what it can say:
/// `out of memory[pid]`, with no header, whatever `LOG_PID` says.
#[test]
fn out_of_memory_says_so_in_place_of_the_record() {
    let _g = fresh(glibc_world("dgram"));
    world::with(|w| w.alloc_fails = true);
    openlog(b"tag\0".as_ptr(), LOG_PERROR, LOG_USER);
    log(LOG_INFO, &[b'z'; 2000]);
    let (sent, stderr, _) = look();
    assert_eq!(sent, [b"out of memory[4242]".to_vec()]);
    assert_eq!(stderr, b"out of memory[4242]\n");
}

/// When `localtime_r` fails, the header is the priority alone -- no
/// timestamp, no tag -- and standard error's copy is the message alone.
#[test]
fn without_a_timestamp_the_header_is_the_priority_alone() {
    let _g = fresh(glibc_world("dgram"));
    world::with(|w| w.clock = None);
    openlog(b"tag\0".as_ptr(), LOG_PERROR | LOG_PID, LOG_USER);
    log(LOG_INFO, b"bare");
    let (sent, stderr, _) = look();
    assert_eq!(sent, [b"<14>: bare".to_vec()]);
    assert_eq!(stderr, b"bare\n");
}

/// With no program name, the tag is `%s`'s `(null)`.
#[test]
fn a_null_program_name_is_spelled_as_printf_spells_it() {
    let _g = fresh(glibc_world("dgram"));
    world::with(|w| w.progname = None);
    log(LOG_INFO, b"m");
    let (sent, _, _) = look();
    assert_eq!(sent, [record(14, b"(null): m")]);
}

/// The caller's `va_list` is read from a copy, as `va_copy` gives glibc
/// one: it is as it was after the call.
#[test]
fn the_callers_va_list_is_left_as_it_was() {
    let _g = fresh(glibc_world("dgram"));
    let text = b"str\0";
    with_va(&[7, text.as_ptr() as u64], &[2.5], |va| {
        // SAFETY: `va` is the live va_list.
        let before = unsafe { ((*va).gp_offset, (*va).fp_offset, (*va).overflow_arg_area) };
        // SAFETY: the format converts what the va_list holds.
        unsafe { vsyslog(LOG_INFO, b"%d %s %.1f\0".as_ptr(), va) };
        // SAFETY: `va` is the live va_list.
        let after = unsafe { ((*va).gp_offset, (*va).fp_offset, (*va).overflow_arg_area) };
        assert_eq!(before, after);
    });
    let (sent, _, _) = look();
    assert_eq!(sent, [record(14, b"syslog-oracle: 7 str 2.5")]);
}

/// A null format logs nothing (glibc's `vsnprintf` refuses it); a null
/// `va_list` reads zeros.
#[test]
fn a_null_format_logs_nothing_and_a_null_va_list_reads_zeros() {
    let _g = fresh(glibc_world("dgram"));
    // SAFETY: a null format and a null va_list are both accepted.
    unsafe { vsyslog(LOG_INFO, core::ptr::null(), core::ptr::null_mut()) };
    assert!(look().0.is_empty());
    // SAFETY: as above; `%d` reads zero from the null va_list.
    unsafe { vsyslog(LOG_INFO, b"n=%d\0".as_ptr(), core::ptr::null_mut()) };
    assert_eq!(look().0, [record(14, b"syslog-oracle: n=0")]);
}

/// The complaint spells the whole priority in hex, as `%x` does.
#[test]
fn the_complaint_spells_the_priority_in_hex() {
    for (pri, hex) in [(0x400 | LOG_INFO, "406"), (i32::MIN, "80000000"), (0x7fff_ffff, "7fffffff")] {
        let _g = fresh(glibc_world("dgram"));
        log(pri, b"m");
        let (sent, _, _) = look();
        let want = format!("syslog-oracle: syslog: unknown facility/priority: {hex}");
        assert_eq!(sent[0], record(35, want.as_bytes()), "{pri:#x}");
        assert_eq!(sent.len(), 2);
    }
}

/// Records from many threads reach the daemon whole and all of them, while
/// another thread opens and closes the logger.
#[test]
fn records_from_many_threads_arrive_whole() {
    let _g = fresh(glibc_world("dgram"));
    std::thread::scope(|s| {
        for t in 0..8u8 {
            s.spawn(move || {
                for i in 0..50u8 {
                    log(LOG_INFO, format!("thread {t} record {i}").as_bytes());
                }
            });
        }
        s.spawn(|| {
            for _ in 0..50 {
                openlog(core::ptr::null(), LOG_NDELAY, LOG_USER);
                closelog();
            }
        });
    });
    let (sent, _, _) = look();
    assert_eq!(sent.len(), 400);
    for rec in &sent {
        let text = mask_stamps(rec);
        assert!(
            text.starts_with(b"<14>@@@@@@@@@@@@@@@@syslog-oracle: thread "),
            "{:?}",
            std::str::from_utf8(&text)
        );
    }
}

// ---------------------------------------------------------------------------
// The journal
// ---------------------------------------------------------------------------

/// With no Unix-domain sockets, a record is a journal line, in
/// `journalrec`'s spelling, and `errno` is as it was.
#[test]
fn without_unix_sockets_a_record_is_a_journal_line() {
    let _g = fresh(slate_world());
    openlog(b"tag\0".as_ptr(), LOG_PID, LOG_DAEMON);
    errno::set_errno(77);
    log(LOG_ERR, b"it works");
    assert_eq!(errno::get_errno(), 77);
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
    assert_eq!(
        journal.as_deref(),
        Some(journalrec_line("err", "tag", "it works", 4242, Some("daemon")).as_slice())
    );
    assert_eq!(
        journal.as_deref(),
        Some(
            &br#"{"ts":1790843180,"level":"err","service":"tag","msg":"it works","pid":4242,"facility":"daemon"}
"#[..]
        )
    );
    let (sent, stderr, console) = look();
    assert!(sent.is_empty() && stderr.is_empty() && console.is_empty());
    assert_eq!(world::with(|w| w.open_descriptors()), 0, "the journal is closed again");
}

/// Every message is escaped as `journalrec::escape` escapes it, byte for
/// byte; one newline at the end is the line's end, not the message's.
#[test]
fn a_record_is_escaped_as_journalrec_escapes_it() {
    let cases: [(&[u8], &str); 16] = [
        (b"plain", "plain"),
        (b"a \"quoted\" word", "a \"quoted\" word"),
        (b"back\\slash", "back\\slash"),
        (b"two\nlines", "two\nlines"),
        (b"cr\rtab\t", "cr\rtab\t"),
        (b"bell\x07 esc\x1b unit\x1f", "bell\u{7} esc\u{1b} unit\u{1f}"),
        (b"del\x7f", "del\u{7f}"),
        ("caf\u{e9} \u{65e5}\u{672c} \u{2028}".as_bytes(), "caf\u{e9} \u{65e5}\u{672c} \u{2028}"),
        (b"ends in a newline\n", "ends in a newline"),
        (b"ends in two\n\n", "ends in two\n"),
        (b"\n", ""),
        (b"", ""),
        (b"{\"ts\":0}", "{\"ts\":0}"),
        (b"\\u0000", "\\u0000"),
        (b"   spaces   ", "   spaces   "),
        (b"%%", "%%"),
    ];
    for (msg, want) in cases {
        let _g = fresh(slate_world());
        log(LOG_INFO, msg);
        let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
        let expected = journalrec_line("info", "syslog-oracle", want, 4242, Some("user"));
        assert_eq!(
            journal.as_deref(),
            Some(expected.as_slice()),
            "{:?}",
            std::str::from_utf8(msg)
        );
    }
}

/// A NUL in the message (`%c` of 0) is a control character like any other.
#[test]
fn an_embedded_nul_is_escaped() {
    let _g = fresh(slate_world());
    with_va(&[0], &[], |va| {
        // SAFETY: `%c` reads the one integer the va_list holds.
        unsafe { vsyslog(LOG_INFO, b"a%cb\0".as_ptr(), va) };
    });
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
    let expected = journalrec_line("info", "syslog-oracle", "a\u{0}b", 4242, Some("user"));
    assert_eq!(journal.as_deref(), Some(expected.as_slice()));
}

/// The facility as glibc's `facilitynames` names it, first name first, and
/// none for a facility without a name; the level as `journalctl` reads it.
#[test]
fn the_facility_and_level_are_named_as_glibc_names_them() {
    let cases: [(Option<i32>, i32, &str, Option<&str>); 10] = [
        (Some(0), LOG_INFO, "info", Some("kern")),
        (None, LOG_AUTH | LOG_NOTICE, "notice", Some("auth")),
        (None, LOG_AUTHPRIV | LOG_WARNING, "warning", Some("authpriv")),
        (None, LOG_FTP | LOG_CRIT, "crit", Some("ftp")),
        (None, (12 << 3) | LOG_ALERT, "alert", None),
        (None, (24 << 3) | LOG_EMERG, "emerg", Some("mark")),
        (None, (25 << 3) | LOG_DEBUG, "debug", None),
        (None, LOG_LOCAL7 | LOG_ERR, "err", Some("local7")),
        (None, LOG_INFO, "info", Some("user")),
        (Some(LOG_CRON), LOG_INFO, "info", Some("cron")),
    ];
    for (facility, pri, level, name) in cases {
        let _g = fresh(slate_world());
        if let Some(f) = facility {
            openlog(core::ptr::null(), 0, f);
        }
        log(pri, b"m");
        let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
        let expected = journalrec_line(level, "syslog-oracle", "m", 4242, name);
        assert_eq!(journal.as_deref(), Some(expected.as_slice()), "{pri:#x}");
    }
}

/// The record carries the process's PID whether or not `LOG_PID` asked the
/// header to: it is what a daemon learns from the socket.
#[test]
fn a_record_carries_the_pid_without_log_pid() {
    let _g = fresh(slate_world());
    world::with(|w| w.pid = 99);
    log(LOG_INFO, b"m");
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec)).expect("a journal");
    assert!(journal.windows(10).any(|w| w == b",\"pid\":99,"), "{journal:?}");
}

/// A clock before the epoch is stamped 0, as `logger` stamps it.
#[test]
fn a_clock_before_the_epoch_is_stamped_zero() {
    let _g = fresh(slate_world());
    world::with(|w| w.now = -5);
    log(LOG_INFO, b"m");
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec)).expect("a journal");
    assert!(journal.starts_with(b"{\"ts\":0,"), "{journal:?}");
}

/// A message or tag that is not UTF-8 goes to standard error -- once, not
/// again under `LOG_PERROR` -- and the journal is left alone.
#[test]
fn a_record_that_is_not_utf8_goes_to_stderr_once() {
    let _g = fresh(slate_world());
    log(LOG_INFO, b"byte \xff here");
    let (_, stderr, _) = look();
    assert_eq!(stderr, b"syslog-oracle: byte \xff here\n");

    openlog(b"tag\0".as_ptr(), LOG_PERROR, LOG_USER);
    log(LOG_INFO, b"again \xfe\n");
    let (_, stderr, _) = look();
    assert_eq!(stderr, b"tag: again \xfe\n");

    openlog(b"t\xff\0".as_ptr(), 0, LOG_USER);
    log(LOG_INFO, b"fine");
    let (_, stderr, _) = look();
    assert_eq!(stderr, b"t\xff: fine\n");
    assert_eq!(world::with(|w| w.journal().map(<[u8]>::to_vec)), None);
}

/// A journal that cannot be opened is a send that failed: tried again on a
/// new connection, then `LOG_CONS`; and tried afresh on the next record.
#[test]
fn a_journal_that_cannot_be_opened_is_retried_then_the_console() {
    let _g = fresh(slate_world());
    world::with(|w| w.open_errors = vec![errno::ENOENT, errno::ENOENT]);
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_ERR, b"to the console");
    let (_, _, console) = look();
    assert_eq!(console, b"tag: to the console\r\n");
    let opens = world::with(|w| w.trace.iter().filter(|t| t.starts_with("open /var")).count());
    assert_eq!(opens, 2);
    log(LOG_INFO, b"later");
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
    assert_eq!(
        journal.as_deref(),
        Some(journalrec_line("info", "tag", "later", 4242, Some("user")).as_slice())
    );
}

/// Without `LOG_CONS` a record the journal will not take is lost, as glibc
/// loses one its daemon will not take.
#[test]
fn without_log_cons_a_failed_record_is_lost() {
    let _g = fresh(slate_world());
    world::with(|w| w.open_errors = vec![errno::EACCES, errno::EACCES]);
    log(LOG_ERR, b"lost");
    let (sent, stderr, console) = look();
    assert!(sent.is_empty() && stderr.is_empty() && console.is_empty());
}

/// The journal's lock: taken again when a signal interrupts the wait; done
/// without where there are no locks; a failure otherwise.
#[test]
fn the_lock_is_retried_after_a_signal_and_skipped_without_locks() {
    let _g = fresh(slate_world());
    world::with(|w| w.flock_errors = vec![errno::EINTR, errno::EINTR]);
    log(LOG_INFO, b"after two signals");
    let flocks = world::with(|w| w.trace.iter().filter(|t| t.starts_with("flock")).count());
    assert_eq!(flocks, 3);
    assert!(world::with(|w| w.journal().is_some()));

    for e in [errno::ENOLCK, errno::ENOSYS, errno::EOPNOTSUPP] {
        world::with(|w| {
            w.flock_errors = vec![e];
            w.trace.clear();
        });
        log(LOG_INFO, b"no locks");
        let trace = world::with(|w| w.trace.clone());
        assert!(!trace.iter().any(|t| t.starts_with("same")), "{e}: {trace:?}");
        assert!(trace.iter().any(|t| t.starts_with("write")), "{e}: {trace:?}");
    }

    world::with(|w| w.flock_errors = vec![errno::EIO, errno::EIO]);
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_INFO, b"no lock, no record");
    let (_, _, console) = look();
    assert_eq!(console, b"tag: no lock, no record\r\n");
}

/// A journal rotated away while the lock was awaited: the record goes to
/// the new file the path names after, not the rotated one.
#[test]
fn a_journal_rotated_while_waiting_gets_the_record_in_the_new_file() {
    let _g = fresh(slate_world());
    log(LOG_INFO, b"first");
    let old = world::with(|w| *w.paths.values().next().expect("a journal"));
    world::with(|w| w.rotations = 1);
    log(LOG_INFO, b"second");
    world::with(|w| {
        let new = *w.paths.values().next().expect("a new journal");
        assert_ne!(new, old);
        assert_eq!(
            w.inodes[&old],
            journalrec_line("info", "syslog-oracle", "first", 4242, Some("user"))
        );
        assert_eq!(
            w.inodes[&new],
            journalrec_line("info", "syslog-oracle", "second", 4242, Some("user"))
        );
    });
}

/// A journal rewritten while the lock was awaited: the record goes after
/// the rewrite's contents, in the file that holds them.
#[test]
fn a_journal_rewritten_while_waiting_gets_the_record_after_the_rewrite() {
    let _g = fresh(slate_world());
    world::with(|w| {
        w.rewrites = 1;
        w.rewrite_with = b"kept\n".to_vec();
    });
    log(LOG_INFO, b"after");
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec)).expect("a journal");
    let mut want = b"kept\n".to_vec();
    want.extend(journalrec_line("info", "syslog-oracle", "after", 4242, Some("user")));
    assert_eq!(journal, want);
    assert_eq!(world::with(|w| w.open_descriptors()), 0);
}

/// A journal that keeps being replaced is written after 64 tries, to the
/// file the last try holds.
#[test]
fn a_journal_that_keeps_being_replaced_is_written_after_64_tries() {
    let _g = fresh(slate_world());
    world::with(|w| w.rewrites = 1000);
    log(LOG_INFO, b"at last");
    world::with(|w| {
        let opens = w.trace.iter().filter(|t| t.starts_with("open /var")).count();
        assert_eq!(opens, 64);
        let line = journalrec_line("info", "syslog-oracle", "at last", 4242, Some("user"));
        assert_eq!(w.inodes.values().filter(|c| **c == line).count(), 1);
        assert_eq!(w.open_descriptors(), 0);
    });
}

/// A stat that fails for a reason other than the path naming nothing is a
/// failed send.
#[test]
fn a_stat_that_fails_is_a_failed_send() {
    let _g = fresh(slate_world());
    world::with(|w| w.stat_errors = vec![errno::EACCES, errno::EACCES]);
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_INFO, b"m");
    let (_, _, console) = look();
    assert_eq!(console, b"tag: m\r\n");
    assert_eq!(world::with(|w| w.open_descriptors()), 0);
}

/// A short write is continued until the record is whole; a write that fails
/// is a failed send.
#[test]
fn a_short_write_is_continued_and_a_failed_one_is_a_failed_send() {
    let _g = fresh(slate_world());
    world::with(|w| w.write_limit = Some(7));
    log(LOG_INFO, b"in pieces");
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
    assert_eq!(
        journal.as_deref(),
        Some(journalrec_line("info", "syslog-oracle", "in pieces", 4242, Some("user")).as_slice())
    );

    world::with(|w| {
        w.write_limit = None;
        w.write_errors = vec![errno::EINTR, errno::ENOSPC, errno::ENOSPC];
    });
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_INFO, b"full");
    let (_, _, console) = look();
    assert_eq!(console, b"tag: full\r\n");
}

/// A record too long for the stack buffer is built on the heap; with no
/// heap for it, it is a failed send.
#[test]
fn a_long_record_is_built_on_the_heap_or_not_at_all() {
    let _g = fresh(slate_world());
    // Under 1024 bytes as a message, over 1024 escaped.
    let msg = vec![b'"'; 900];
    log(LOG_INFO, &msg);
    let journal = world::with(|w| w.journal().map(<[u8]>::to_vec));
    let want = journalrec_line("info", "syslog-oracle", &"\"".repeat(900), 4242, Some("user"));
    assert_eq!(journal.as_deref(), Some(want.as_slice()));

    world::with(|w| {
        w.alloc_fails = true;
        w.paths.clear();
    });
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_INFO, &msg);
    let (_, _, console) = look();
    assert_eq!(console, [b"tag: ".as_slice(), &msg, b"\r\n"].concat());
    assert_eq!(world::with(|w| w.journal().map(<[u8]>::to_vec)), None);
}

/// `openlog` under `LOG_NDELAY` chooses the journal at once and leaves
/// `errno` alone; `closelog` closes no descriptor, there being none; the next
/// record chooses it again.
#[test]
fn the_journal_is_a_connection_without_a_descriptor() {
    let _g = fresh(slate_world());
    errno::set_errno(55);
    openlog(b"tag\0".as_ptr(), LOG_NDELAY, LOG_USER);
    assert_eq!(errno::get_errno(), 55);
    closelog();
    log(LOG_INFO, b"m");
    world::with(|w| {
        assert!(!w.trace.iter().any(|t| t.starts_with("close -1")));
        let sockets = w.trace.iter().filter(|t| t.starts_with("socket")).count();
        assert_eq!(sockets, 2, "{:?}", w.trace);
        assert!(w.journal().is_some());
    });
}

/// Where Unix-domain sockets exist the journal is never used, daemon or no
/// daemon: that is glibc's behaviour, and the journal retires itself.
#[test]
fn with_unix_sockets_the_journal_is_never_used() {
    let _g = fresh(glibc_world("none"));
    log(LOG_INFO, b"lost");
    assert_eq!(world::with(|w| w.journal().map(<[u8]>::to_vec)), None);
}

/// Only `EAFNOSUPPORT` -- no such sockets at all -- makes the journal the
/// log. A socket call that fails for another reason, out of descriptors
/// say, is glibc's failure to connect: the record is lost, or goes to the
/// console, and `errno` says why.
#[test]
fn only_eafnosupport_makes_the_journal_the_log() {
    let _g = fresh(glibc_world("dgram"));
    world::with(|w| w.socket_errors = vec![errno::EMFILE, errno::EMFILE]);
    openlog(b"tag\0".as_ptr(), LOG_CONS, LOG_USER);
    log(LOG_INFO, b"no descriptors");
    let (sent, _, console) = look();
    assert!(sent.is_empty());
    assert_eq!(console, b"tag: no descriptors\r\n");
    assert_eq!(world::with(|w| w.journal().map(<[u8]>::to_vec)), None);
    assert_eq!(errno::get_errno(), errno::EMFILE);
}

// ---------------------------------------------------------------------------
// The interface
// ---------------------------------------------------------------------------

#[test]
fn setlogmask_returns_the_mask_before_and_zero_only_asks() {
    let _g = fresh(glibc_world("dgram"));
    assert_eq!(setlogmask(log_upto(LOG_ERR)), 0xff);
    assert_eq!(setlogmask(0), log_upto(LOG_ERR));
    assert_eq!(setlogmask(0), log_upto(LOG_ERR));
    assert_eq!(setlogmask(log_mask(LOG_DEBUG)), log_upto(LOG_ERR));
    log(LOG_INFO, b"masked");
    log(LOG_DEBUG, b"kept");
    assert_eq!(look().0, [record(15, b"syslog-oracle: kept")]);
}

#[test]
fn closelog_keeps_the_options_facility_and_mask() {
    let _g = fresh(glibc_world("dgram"));
    openlog(b"tag\0".as_ptr(), LOG_PID, LOG_LOCAL1);
    setlogmask(log_upto(LOG_NOTICE));
    closelog();
    log(LOG_NOTICE, b"m");
    log(LOG_INFO, b"masked");
    assert_eq!(look().0, [record(LOG_LOCAL1 | LOG_NOTICE, b"syslog-oracle[4242]: m")]);
}

#[test]
fn the_values_are_glibcs() {
    assert_eq!(
        [LOG_EMERG, LOG_ALERT, LOG_CRIT, LOG_ERR, LOG_WARNING, LOG_NOTICE, LOG_INFO, LOG_DEBUG],
        [0, 1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(
        [LOG_KERN, LOG_USER, LOG_MAIL, LOG_DAEMON, LOG_AUTH, LOG_SYSLOG, LOG_LPR, LOG_NEWS],
        [0, 8, 16, 24, 32, 40, 48, 56]
    );
    assert_eq!([LOG_UUCP, LOG_CRON, LOG_AUTHPRIV, LOG_FTP], [64, 72, 80, 88]);
    assert_eq!(
        [LOG_LOCAL0, LOG_LOCAL1, LOG_LOCAL2, LOG_LOCAL3, LOG_LOCAL4, LOG_LOCAL5, LOG_LOCAL6, LOG_LOCAL7],
        [128, 136, 144, 152, 160, 168, 176, 184]
    );
    assert_eq!(
        [LOG_PID, LOG_CONS, LOG_ODELAY, LOG_NDELAY, LOG_NOWAIT, LOG_PERROR],
        [1, 2, 4, 8, 16, 32]
    );
    assert_eq!((LOG_PRIMASK, LOG_FACMASK, LOG_NFACILITIES), (7, 0x3f8, 24));
    assert_eq!(INTERNALLOG, 0x23);
}

#[test]
fn log_mask_and_log_upto_are_the_macros() {
    for p in 0..=7 {
        assert_eq!(log_mask(p), 1 << p);
        assert_eq!(log_upto(p), (1 << (p + 1)) - 1);
        assert_eq!(log_pri(p | LOG_LOCAL7), p);
    }
}

#[test]
fn the_facility_table_is_glibcs() {
    let names: Vec<&[u8]> = FACILITY_NAMES.iter().map(|&(n, _)| n).collect();
    assert_eq!(
        names,
        [
            &b"auth"[..], b"authpriv", b"cron", b"daemon", b"ftp", b"kern", b"lpr", b"mail", b"mark",
            b"news", b"security", b"syslog", b"user", b"uucp", b"local0", b"local1", b"local2",
            b"local3", b"local4", b"local5", b"local6", b"local7",
        ]
    );
    let names: Vec<&str> = PRIORITY_NAMES
        .iter()
        .map(|n| std::str::from_utf8(n).expect("ASCII"))
        .collect();
    assert_eq!(names, journalrec::PRIORITY_NAMES);
    assert_eq!(
        std::str::from_utf8(JOURNAL_PATH.strip_suffix(b"\0").expect("NUL")),
        Ok(journalrec::MAIN_LOG_PATH)
    );
}
