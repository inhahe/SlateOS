//! `rcmd`'s system in the host tests: a world of their own, per test thread,
//! that the replay of glibc's answers (`rcmd_oracle.txt`) sets up a line at
//! a time. Its server is the oracle harness's (`rcmd_harness.py`): it
//! listens on a family and port, reads the stderr channel's port, connects
//! back from the port it is given (or writes a byte instead, or closes),
//! reads the three strings, and answers -- what it received recorded as the
//! harness records it. Files are a table of paths; standard error is a
//! buffer; a sleep is a number in a list.

use super::{Buf, Netrc, Storage};
use crate::errno;
use crate::poll::{POLLIN, Pollfd};
use crate::signal::SigsetT;
use crate::socket::SocklenT;
use core::cell::RefCell;
use std::vec::Vec;

/// The scripted server: the harness's `struct srv`.
#[derive(Clone, Debug, Default)]
pub(super) struct Server {
    /// `AF_INET` or `AF_INET6`.
    pub family: i32,
    pub port: u16,
    /// The port to connect the stderr channel back from; 0 any; -1 write a
    /// byte on the connection instead; -2 close it.
    pub back: i32,
    /// Whether it reads the three strings before it answers.
    pub strings: bool,
    pub reply: Vec<u8>,
    pub data: Vec<u8>,
    pub errdata: Vec<u8>,
}

/// A file: its text, mode, kind (`f`, `d` or `l`), links and owner.
#[derive(Clone, Debug)]
pub(super) struct FileSpec {
    pub text: Vec<u8>,
    pub mode: u32,
    pub kind: u8,
    pub links: u32,
    pub uid: u32,
}

#[derive(Default)]
pub(super) struct Sock {
    id: i32,
    family: i32,
    port: u16,
    listening: bool,
    /// For a listener: connections not yet accepted.
    pending: Vec<i32>,
    /// For a connection accepted: the port its peer is at.
    peer_port: u16,
    inbox: Vec<u8>,
    eof: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    #[default]
    Idle,
    /// Reading the stderr channel's port, to its NUL.
    Port,
    /// Reading the strings: this many NULs to go.
    Strings(u32),
    Done,
}

/// The world.
#[derive(Default)]
pub(super) struct World {
    /// None: nothing listening anywhere.
    pub server: Option<Server>,
    /// Ports bound before the call.
    pub busy: Vec<u16>,
    /// Whether a port below 1024 may be bound.
    pub privileged: bool,
    pub files: Vec<(Vec<u8>, FileSpec)>,
    pub euid: u32,
    pub hostname: Vec<u8>,
    /// What the server received, as the harness records it.
    pub recv: Vec<u8>,
    /// The port the client connected from: -1 for `rexec`'s, 0 none.
    pub cport: i32,
    pub stderr: Vec<u8>,
    pub sleeps: Vec<u32>,
    pub(super) socks: Vec<Sock>,
    pub(super) next_fd: i32,
    pub(super) next_ephemeral: u16,
    pub(super) phase: Phase,
    pub(super) main: Option<i32>,
    pub(super) back_end: Option<i32>,
}

std::thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

/// Make `world` this thread's.
pub(super) fn set(world: World) {
    WORLD.with(|w| {
        let mut w = w.borrow_mut();
        *w = world;
        w.next_fd = 100;
        w.next_ephemeral = 40000;
    });
}

/// Read this thread's world.
pub(super) fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|w| f(&mut w.borrow_mut()))
}

impl World {
    fn sock(&mut self, fd: i32) -> Option<&mut Sock> {
        self.socks.iter_mut().find(|s| s.id == fd)
    }

    fn new_sock(&mut self, family: i32) -> i32 {
        let id = self.next_fd;
        self.next_fd += 1;
        self.socks.push(Sock {
            id,
            family,
            ..Sock::default()
        });
        id
    }

    fn ephemeral(&mut self) -> u16 {
        let p = self.next_ephemeral;
        self.next_ephemeral += 1;
        p
    }

    /// A byte from the client on the connection to the server.
    fn feed(&mut self, b: u8) {
        match self.phase {
            Phase::Port => {
                self.recv.push(b);
                if b == 0 {
                    self.port_received();
                }
            }
            Phase::Strings(n) => {
                self.recv.push(b);
                if b == 0 {
                    if n <= 1 {
                        self.answer();
                    } else {
                        self.phase = Phase::Strings(n - 1);
                    }
                }
            }
            Phase::Idle | Phase::Done => {}
        }
    }

    /// The stderr channel's port has come: connect back, or not.
    fn port_received(&mut self) {
        let Some(server) = self.server.clone() else {
            return;
        };
        let digits: Vec<u8> = self.recv.iter().copied().take_while(|&b| b != 0).collect();
        let port: u32 = core::str::from_utf8(&digits)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if server.back == 0 && port != 0 {
            // The harness's: the digits of a port any one, as one `E`.
            self.recv = std::vec![b'E', 0];
        }
        if port != 0 {
            match server.back {
                -1 => {
                    if let Some(m) = self.main.and_then(|m| self.sock(m).map(|s| s.id)) {
                        if let Some(s) = self.sock(m) {
                            s.inbox.push(b'x');
                        }
                    }
                }
                -2 => {
                    if let Some(m) = self.main {
                        if let Some(s) = self.sock(m) {
                            s.eof = true;
                        }
                    }
                    self.phase = Phase::Done;
                    return;
                }
                back => {
                    let from = if back > 0 {
                        u16::try_from(back).unwrap_or(0)
                    } else {
                        self.ephemeral()
                    };
                    let listener = self
                        .socks
                        .iter()
                        .find(|s| s.listening && u32::from(s.port) == port)
                        .map(|s| (s.id, s.family, s.port));
                    let Some((lid, family, lport)) = listener else {
                        panic!("the server connects back to {port}, where nothing listens");
                    };
                    let end = self.new_sock(family);
                    if let Some(s) = self.sock(end) {
                        s.port = lport;
                        s.peer_port = from;
                    }
                    if let Some(l) = self.sock(lid) {
                        l.pending.push(end);
                    }
                    self.back_end = Some(end);
                }
            }
        }
        self.phase = if server.strings {
            Phase::Strings(3)
        } else {
            Phase::Done
        };
        if !server.strings && server.back != -1 {
            self.answer();
        }
    }

    /// The strings have come: the reply, the data, and both channels closed.
    fn answer(&mut self) {
        self.phase = Phase::Done;
        let Some(server) = self.server.clone() else {
            return;
        };
        if server.back == -1 {
            return;
        }
        let ok = server.reply.first() == Some(&0);
        if let Some(m) = self.main {
            if let Some(s) = self.sock(m) {
                s.inbox.extend_from_slice(&server.reply);
                if ok {
                    s.inbox.extend_from_slice(&server.data);
                }
                s.eof = true;
            }
        }
        if let Some(e) = self.back_end {
            if let Some(s) = self.sock(e) {
                if ok {
                    s.inbox.extend_from_slice(&server.errdata);
                }
                s.eof = true;
            }
        }
    }
}

fn fail(e: i32) -> i32 {
    errno::set_errno(e);
    -1
}

pub(super) fn socket(family: i32) -> i32 {
    with(|w| w.new_sock(family))
}

pub(super) fn bind(fd: i32, sa: &Storage, _len: SocklenT) -> i32 {
    with(|w| {
        let port = sa.port();
        let taken = w.busy.contains(&port)
            || w.socks
                .iter()
                .any(|s| s.id != fd && s.port == port && port != 0);
        if taken {
            return fail(errno::EADDRINUSE);
        }
        if port < 1024 && !w.privileged {
            return fail(errno::EACCES);
        }
        match w.sock(fd) {
            Some(s) => {
                s.port = port;
                0
            }
            None => fail(errno::EBADF),
        }
    })
}

pub(super) fn connect(fd: i32, sa: *const crate::socket::Sockaddr, _len: SocklenT) -> i32 {
    // SAFETY: `getaddrinfo`'s address: its family and port are its first
    // four bytes.
    let head = unsafe { core::slice::from_raw_parts(sa.cast::<u8>(), 4) };
    let family = i32::from(u16::from_ne_bytes([head[0], head[1]]));
    let port = u16::from_be_bytes([head[2], head[3]]);
    with(|w| {
        let unbound = w.sock(fd).is_some_and(|s| s.port == 0);
        if unbound {
            let p = w.ephemeral();
            if let Some(s) = w.sock(fd) {
                s.port = p;
            }
        }
        let listening = w
            .server
            .as_ref()
            .is_some_and(|s| s.family == family && s.port == port);
        if !listening {
            return fail(errno::ECONNREFUSED);
        }
        let rexec = w.server.as_ref().is_some_and(|s| s.back == 0);
        let own = w.sock(fd).map_or(0, |s| s.port);
        w.cport = if rexec { -1 } else { i32::from(own) };
        w.main = Some(fd);
        w.phase = Phase::Port;
        w.recv.clear();
        0
    })
}

pub(super) fn listen(fd: i32, _backlog: i32) -> i32 {
    with(|w| {
        let unbound = w.sock(fd).is_some_and(|s| s.port == 0);
        let p = if unbound { w.ephemeral() } else { 0 };
        match w.sock(fd) {
            Some(s) => {
                if unbound {
                    s.port = p;
                }
                s.listening = true;
                0
            }
            None => fail(errno::EBADF),
        }
    })
}

pub(super) fn accept(fd: i32, sa: &mut Storage, len: &mut SocklenT) -> i32 {
    with(|w| {
        let Some(l) = w.sock(fd) else {
            return fail(errno::EBADF);
        };
        if l.pending.is_empty() {
            return fail(errno::EAGAIN);
        }
        let end = l.pending.remove(0);
        let (family, peer) = w.sock(end).map_or((0, 0), |s| (s.family, s.peer_port));
        *sa = Storage::ZERO;
        sa.set_family(family);
        sa.set_port(peer);
        *len = super::sa_len(family);
        end
    })
}

pub(super) fn getsockname(fd: i32, sa: &mut Storage, len: &mut SocklenT) -> i32 {
    with(|w| match w.sock(fd) {
        Some(s) => {
            *sa = Storage::ZERO;
            sa.set_family(s.family);
            sa.set_port(s.port);
            *len = super::sa_len(s.family);
            0
        }
        None => fail(errno::EBADF),
    })
}

pub(super) fn poll(fds: &mut [Pollfd; 2], _timeout: i32) -> i32 {
    with(|w| {
        let mut n = 0;
        for p in fds.iter_mut() {
            p.revents = 0;
            if let Some(s) = w.sock(p.fd) {
                let ready = if s.listening {
                    !s.pending.is_empty()
                } else {
                    !s.inbox.is_empty() || s.eof
                };
                if ready {
                    p.revents = POLLIN;
                    n += 1;
                }
            }
        }
        n
    })
}

pub(super) fn read(fd: i32, buf: &mut [u8]) -> isize {
    with(|w| {
        let Some(s) = w.sock(fd) else {
            return fail(errno::EBADF) as isize;
        };
        if s.inbox.is_empty() {
            return if s.eof {
                0
            } else {
                fail(errno::EAGAIN) as isize
            };
        }
        let n = buf.len().min(s.inbox.len());
        buf[..n].copy_from_slice(&s.inbox[..n]);
        s.inbox.drain(..n);
        n as isize
    })
}

pub(super) fn write(fd: i32, buf: &[u8]) -> isize {
    with(|w| {
        if w.main == Some(fd) {
            for &b in buf {
                w.feed(b);
            }
        }
        buf.len() as isize
    })
}

pub(super) fn writev(fd: i32, parts: &[&[u8]; 3]) -> isize {
    parts.iter().map(|p| write(fd, p)).sum()
}

pub(super) fn close(fd: i32) {
    with(|w| {
        if w.main == Some(fd) {
            // The server reads its end of the file, and is done.
            if w.phase != Phase::Done {
                w.phase = Phase::Done;
            }
            w.main = None;
        }
        w.socks.retain(|s| s.id != fd);
    });
}

pub(super) fn set_owner(_fd: i32, _pid: i32) {}

pub(super) fn sleep(secs: u32) {
    with(|w| w.sleeps.push(secs));
}

pub(super) fn getpid() -> i32 {
    4242
}

pub(super) fn block_sigurg() -> SigsetT {
    SigsetT::EMPTY
}

pub(super) fn restore_mask(_old: &SigsetT) {}

pub(super) fn geteuid() -> u32 {
    with(|w| w.euid)
}

pub(super) fn seteuid(uid: u32) -> i32 {
    with(|w| w.euid = uid);
    0
}

pub(super) fn hostname(buf: &mut [u8; 1024]) -> bool {
    with(|w| {
        let n = w.hostname.len().min(1023);
        buf[..n].copy_from_slice(&w.hostname[..n]);
        buf[n] = 0;
    });
    true
}

pub(super) fn eprint(parts: &[&[u8]]) {
    with(|w| {
        for p in parts {
            w.stderr.extend_from_slice(p);
        }
    });
}

pub(super) fn eprint_raw(c: u8) {
    with(|w| w.stderr.push(c));
}

fn strerror_bytes() -> Vec<u8> {
    let p = crate::string::strerror(errno::get_errno());
    // SAFETY: `strerror` answers a C string.
    unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) }.to_vec()
}

pub(super) fn perror(s: *const u8) {
    let text = strerror_bytes();
    with(|w| {
        // SAFETY: NULL or a C string.
        if !s.is_null() && unsafe { *s } != 0 {
            // SAFETY: a C string.
            let prefix = unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) };
            w.stderr.extend_from_slice(prefix);
            w.stderr.extend_from_slice(b": ");
        }
        w.stderr.extend_from_slice(&text);
        w.stderr.push(b'\n');
    });
}

/// `warnx`'s line, as the harness's program, `rcprobe`, says it.
pub(super) fn warnx(msg: &[&[u8]]) {
    with(|w| {
        w.stderr.extend_from_slice(b"rcprobe: ");
        for p in msg {
            w.stderr.extend_from_slice(p);
        }
        w.stderr.push(b'\n');
    });
}

pub(super) fn warn(msg: &[u8]) {
    let text = strerror_bytes();
    warnx(&[msg, b": ", &text]);
}

fn find(path: &[u8]) -> Option<FileSpec> {
    let path = path.strip_suffix(b"\0").unwrap_or(path);
    with(|w| {
        w.files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, f)| f.clone())
    })
}

pub(super) fn hosts_file(path: &[u8], okuser: u32) -> Result<Buf, &'static [u8]> {
    let Some(f) = find(path) else {
        return Err(b"lstat failed\0");
    };
    if f.kind != b'f' {
        return Err(b"not regular file\0");
    }
    if f.uid != 0 && f.uid != okuser {
        return Err(b"bad owner\0");
    }
    if f.mode & 0o022 != 0 {
        return Err(b"writeable by other than owner\0");
    }
    if f.links > 1 {
        return Err(b"hard linked somewhere\0");
    }
    Buf::copy_of(&f.text).ok_or(b"cannot open\0")
}

pub(super) fn netrc(path: &[u8]) -> Netrc {
    match find(path) {
        None => Netrc::Missing,
        // A directory opens, and reads as nothing, its one read failing.
        Some(f) if f.kind == b'd' => Netrc::Text(Buf::EMPTY, f.mode),
        Some(f) => match Buf::copy_of(&f.text) {
            Some(b) => Netrc::Text(b, f.mode),
            None => Netrc::Error(errno::ENOMEM),
        },
    }
}
