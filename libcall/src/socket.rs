//! Who is at the other end of a Unix-domain socket, as the kernel vouches for
//! it: the process that made the connection (`SO_PEERCRED`) and -- once the
//! socket asks for it (`SO_PASSCRED`) -- the process that wrote each message
//! (`SCM_CREDENTIALS`, which `recvmsg` hands back beside the data). Through
//! the linked C library.
//!
//! Two programs read them, each to file a message under the process that
//! wrote it rather than the one it names: `syslogd`, for each datagram sent to
//! `/dev/log`, and `systemd-cat`'s helper, for each line a command -- or a
//! process the command started -- writes to its standard output. Both do it
//! as systemd-journald does.
//!
//! # Descriptors sent with a message are closed
//!
//! A writer may attach open descriptors to anything it sends (`SCM_RIGHTS`),
//! and `recvmsg` installs them in the reader. Neither reader here wants them,
//! and one that kept them would hold open whatever its writers chose -- so
//! [`receive`] asks for them close-on-exec and closes every one it is given,
//! as journald's `cmsg_close_all` does.
//!
//! # A library that does not say
//!
//! A C library may take `SO_PASSCRED` and still hand back no credentials, or
//! refuse the option outright -- SlateOS's did both before its Unix-domain
//! sockets were finished. A caller has to treat "no sender" as an answer:
//! [`Received::sender`] is an `Option`, and [`pass_credentials`] failing is
//! the caller's cue to read without asking.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `SOL_SOCKET`: the socket's own options, not a protocol's.
#[cfg_attr(not(unix), allow(dead_code))]
const SOL_SOCKET: i32 = 1;
/// Ask for each sender's credentials with what it sent.
#[cfg(any(unix, test))]
const SO_PASSCRED: i32 = 16;
/// The credentials of the process that made the connection.
#[cfg(any(unix, test))]
const SO_PEERCRED: i32 = 17;
/// A control message carrying open descriptors.
#[cfg_attr(not(unix), allow(dead_code))]
const SCM_RIGHTS: i32 = 1;
/// A control message carrying a `struct ucred`.
#[cfg_attr(not(unix), allow(dead_code))]
const SCM_CREDENTIALS: i32 = 2;
/// In the flags `recvmsg` hands back: the datagram was longer than the buffer.
#[cfg(any(unix, test))]
const MSG_TRUNC: i32 = 0x20;
/// `recvmsg`: install any descriptors received close-on-exec.
#[cfg(any(unix, test))]
const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;

/// `sizeof (struct cmsghdr)`: `cmsg_len` (a `size_t`), `cmsg_level`,
/// `cmsg_type` -- already a multiple of the alignment, so it is also where
/// the data starts (`CMSG_DATA`).
#[cfg_attr(not(unix), allow(dead_code))]
const CMSG_HEADER: usize = 16;
/// `CMSG_ALIGN`'s unit on x86-64: `sizeof (size_t)`.
#[cfg_attr(not(unix), allow(dead_code))]
const CMSG_ALIGN: usize = 8;
/// `sizeof (struct ucred)`: `pid`, `uid`, `gid`, four bytes each.
#[cfg_attr(not(unix), allow(dead_code))]
const UCRED: usize = 12;
/// The control buffer [`receive`] offers, in bytes: room for the credentials
/// (32) and twenty descriptors behind them. Any a writer sends beyond that
/// the kernel closes itself, and says so with `MSG_CTRUNC`.
#[cfg(unix)]
const CONTROL: usize = 128;

/// A process, as the kernel describes it on a socket: `struct ucred`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ucred {
    /// Its process id, in the reader's namespace.
    pub pid: i32,
    /// Its user id.
    pub uid: u32,
    /// Its group id.
    pub gid: u32,
}

/// What one [`receive`] read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Received {
    /// How many bytes were put in the buffer. For a datagram socket that is
    /// the whole datagram unless [`truncated`](Received::truncated); zero on a
    /// stream socket is its end.
    pub len: usize,
    /// The datagram was longer than the buffer, and the rest of it is lost
    /// (`MSG_TRUNC`).
    pub truncated: bool,
    /// Who wrote what was read, when the socket asked to be told
    /// ([`pass_credentials`]) and the library told it.
    pub sender: Option<Ucred>,
}

#[cfg(unix)]
mod sys {
    /// `struct iovec`.
    #[repr(C)]
    pub struct Iovec {
        pub base: *mut u8,
        pub len: usize,
    }

    /// `struct msghdr`, x86-64 Linux layout: `repr(C)` puts the padding after
    /// `namelen` and `flags` where C does.
    #[repr(C)]
    pub struct Msghdr {
        pub name: *mut u8,
        pub namelen: u32,
        pub iov: *mut Iovec,
        pub iovlen: usize,
        pub control: *mut u8,
        pub controllen: usize,
        pub flags: i32,
    }

    unsafe extern "C" {
        pub fn setsockopt(fd: i32, level: i32, name: i32, value: *const u8, len: u32) -> i32;
        pub fn getsockopt(fd: i32, level: i32, name: i32, value: *mut u8, len: *mut u32) -> i32;
        pub fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize;
        pub fn close(fd: i32) -> i32;
    }
}

/// Ask the kernel to attach each writer's credentials to what `fd` receives:
/// `setsockopt (fd, SOL_SOCKET, SO_PASSCRED, &1, sizeof (int))`.
///
/// Set it before anything is written: what was sent before carries none.
///
/// # Errors
///
/// What `setsockopt` reports -- `ENOTSOCK`, `ENOPROTOOPT` from a library that
/// does not have the option; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn pass_credentials(fd: i32) -> Result<(), i32> {
    pass_credentials_one(fd)
}

#[cfg(unix)]
fn pass_credentials_one(fd: i32) -> Result<(), i32> {
    let on: i32 = 1;
    // SAFETY: `on` is a live `int` and the length given is its size; the
    // library copies it before returning and keeps no pointer to it.
    let rc =
        unsafe { sys::setsockopt(fd, SOL_SOCKET, SO_PASSCRED, (&raw const on).cast::<u8>(), 4) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn pass_credentials_one(_fd: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// The process at the other end of the connected socket `fd`, as it was when
/// the connection was made: `getsockopt (fd, SOL_SOCKET, SO_PEERCRED)`. For
/// either end of a `socketpair`, the process that made the pair.
///
/// # Errors
///
/// What `getsockopt` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn peer_credentials(fd: i32) -> Result<Ucred, i32> {
    peer_credentials_one(fd)
}

#[cfg(unix)]
fn peer_credentials_one(fd: i32) -> Result<Ucred, i32> {
    let mut raw = [0u8; UCRED];
    let mut len: u32 = 12;
    // SAFETY: `raw` is `len` writable bytes and `len` a live `socklen_t`; the
    // library writes at most `len` bytes into the first and the count into
    // the second, and keeps neither pointer.
    let rc =
        unsafe { sys::getsockopt(fd, SOL_SOCKET, SO_PEERCRED, raw.as_mut_ptr(), &raw mut len) };
    if rc != 0 {
        return Err(last_errno());
    }
    ucred(&raw).ok_or(crate::EINVAL)
}

#[cfg(not(unix))]
fn peer_credentials_one(_fd: i32) -> Result<Ucred, i32> {
    Err(ENOSYS)
}

/// Read what `fd` has into `buf`, with its writer's credentials when the
/// kernel attached them: `recvmsg (fd, ..., MSG_CMSG_CLOEXEC)`. Waits for
/// something to read unless `fd` is non-blocking.
///
/// Any descriptors that came with it are closed before this returns.
///
/// # Errors
///
/// What `recvmsg` reports -- `EINTR` is the caller's to retry, `EAGAIN` a
/// non-blocking socket with nothing to read; [`ENOSYS`](crate::ENOSYS) off
/// Unix.
pub fn receive(fd: i32, buf: &mut [u8]) -> Result<Received, i32> {
    receive_one(fd, buf)
}

#[cfg(unix)]
fn receive_one(fd: i32, buf: &mut [u8]) -> Result<Received, i32> {
    let mut iov = sys::Iovec {
        base: buf.as_mut_ptr(),
        len: buf.len(),
    };
    // `u64`s so that the buffer has `cmsghdr`'s alignment, as
    // `CMSG_BUFFER_TYPE` gives journald's.
    let mut control = [0u64; CONTROL / 8];
    let mut msg = sys::Msghdr {
        name: core::ptr::null_mut(),
        namelen: 0,
        iov: &raw mut iov,
        iovlen: 1,
        control: control.as_mut_ptr().cast::<u8>(),
        controllen: CONTROL,
        flags: 0,
    };
    // SAFETY: `msg` points at `iov`, which points at `buf`, and at
    // `control`; all three outlive the call and the lengths given are
    // theirs, so the library writes at most that much into each.
    let n = unsafe { sys::recvmsg(fd, &raw mut msg, MSG_CMSG_CLOEXEC) };
    let Ok(len) = usize::try_from(n) else {
        return Err(last_errno());
    };
    let mut bytes = [0u8; CONTROL];
    for (chunk, word) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(control.iter()) {
        *chunk = word.to_ne_bytes();
    }
    let used = bytes.get(..msg.controllen.min(CONTROL)).unwrap_or_default();
    let sender = parse_control(used, &mut |received| {
        // SAFETY: the kernel installed `received` in this process for this
        // call and nothing else knows its number; closing it here, once, is
        // the only use it gets. Its close cannot usefully fail.
        unsafe { sys::close(received) };
    });
    Ok(Received {
        len,
        truncated: msg.flags & MSG_TRUNC != 0,
        sender,
    })
}

#[cfg(not(unix))]
fn receive_one(_fd: i32, _buf: &mut [u8]) -> Result<Received, i32> {
    Err(ENOSYS)
}

/// A `struct ucred` from its twelve bytes.
#[cfg_attr(not(unix), allow(dead_code))]
fn ucred(bytes: &[u8]) -> Option<Ucred> {
    let word =
        |at: usize| -> Option<[u8; 4]> { bytes.get(at..at.checked_add(4)?)?.try_into().ok() };
    Some(Ucred {
        pid: i32::from_ne_bytes(word(0)?),
        uid: u32::from_ne_bytes(word(4)?),
        gid: u32::from_ne_bytes(word(8)?),
    })
}

/// Walk the control messages `recvmsg` left in `control` -- the bytes it
/// says it used -- the way `CMSG_FIRSTHDR` and `CMSG_NXTHDR` do: the
/// credentials, if there are any, and every descriptor handed to `close`.
///
/// Read field by field from the bytes, so no alignment is assumed; a message
/// whose length runs past the buffer ends the walk, as `CMSG_NXTHDR` ends it.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_control(control: &[u8], close: &mut dyn FnMut(i32)) -> Option<Ucred> {
    let mut sender = None;
    let mut at = 0usize;
    while let Some(header) = at
        .checked_add(CMSG_HEADER)
        .and_then(|end| control.get(at..end))
    {
        let len = header
            .get(..8)
            .and_then(|b| b.try_into().ok())
            .map_or(0, usize::from_ne_bytes);
        let level = header
            .get(8..12)
            .and_then(|b| b.try_into().ok())
            .map_or(0, i32::from_ne_bytes);
        let kind = header
            .get(12..16)
            .and_then(|b| b.try_into().ok())
            .map_or(0, i32::from_ne_bytes);
        if len < CMSG_HEADER {
            break;
        }
        let Some(data) = at
            .checked_add(len)
            .and_then(|end| control.get(at.saturating_add(CMSG_HEADER)..end))
        else {
            break;
        };
        if level == SOL_SOCKET && kind == SCM_CREDENTIALS && data.len() >= UCRED {
            sender = ucred(data);
        } else if level == SOL_SOCKET && kind == SCM_RIGHTS {
            for fd in data.as_chunks::<4>().0 {
                close(i32::from_ne_bytes(*fd));
            }
        }
        // `CMSG_ALIGN (cmsg_len)`.
        let Some(next) = len
            .checked_add(CMSG_ALIGN - 1)
            .map(|l| l & !(CMSG_ALIGN - 1))
            .and_then(|l| at.checked_add(l))
        else {
            break;
        };
        at = next;
    }
    sender
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// One control message, padded as the kernel pads it.
    fn cmsg(level: i32, kind: i32, data: &[u8]) -> Vec<u8> {
        let len = CMSG_HEADER + data.len();
        let mut out = Vec::new();
        out.extend_from_slice(&len.to_ne_bytes());
        out.extend_from_slice(&level.to_ne_bytes());
        out.extend_from_slice(&kind.to_ne_bytes());
        out.extend_from_slice(data);
        while out.len() % CMSG_ALIGN != 0 {
            out.push(0);
        }
        out
    }

    fn creds(pid: i32, uid: u32, gid: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&pid.to_ne_bytes());
        out.extend_from_slice(&uid.to_ne_bytes());
        out.extend_from_slice(&gid.to_ne_bytes());
        out
    }

    /// The restated numbers are the library's.
    #[test]
    fn the_numbers_are_linuxs() {
        assert_eq!(SO_PASSCRED, 16);
        assert_eq!(SO_PEERCRED, 17);
        assert_eq!(MSG_TRUNC, 0x20);
        assert_eq!(MSG_CMSG_CLOEXEC, 0x4000_0000);
    }

    #[test]
    fn credentials_are_read_from_their_message() {
        let control = cmsg(SOL_SOCKET, SCM_CREDENTIALS, &creds(4242, 1000, 100));
        let mut closed = Vec::new();
        let got = parse_control(&control, &mut |fd| closed.push(fd));
        assert_eq!(
            got,
            Some(Ucred {
                pid: 4242,
                uid: 1000,
                gid: 100
            })
        );
        assert!(closed.is_empty());
    }

    #[test]
    fn descriptors_sent_along_are_handed_back_to_be_closed() {
        let mut fds = Vec::new();
        for fd in [7i32, 9, 11] {
            fds.extend_from_slice(&fd.to_ne_bytes());
        }
        let mut control = cmsg(SOL_SOCKET, SCM_CREDENTIALS, &creds(1, 0, 0));
        control.extend(cmsg(SOL_SOCKET, SCM_RIGHTS, &fds));
        let mut closed = Vec::new();
        let got = parse_control(&control, &mut |fd| closed.push(fd));
        assert_eq!(got.map(|c| c.pid), Some(1));
        assert_eq!(closed, [7, 9, 11]);
    }

    #[test]
    fn no_control_data_is_no_sender() {
        assert_eq!(parse_control(&[], &mut |_| {}), None);
    }

    #[test]
    fn a_message_that_overruns_the_buffer_ends_the_walk() {
        let mut control = cmsg(SOL_SOCKET, SCM_CREDENTIALS, &creds(5, 0, 0));
        // Claim more bytes than there are.
        control[..8].copy_from_slice(&999usize.to_ne_bytes());
        assert_eq!(parse_control(&control, &mut |_| {}), None);
        // And a length shorter than its own header.
        control[..8].copy_from_slice(&3usize.to_ne_bytes());
        assert_eq!(parse_control(&control, &mut |_| {}), None);
    }

    #[test]
    fn another_level_is_passed_over() {
        let mut control = cmsg(41, SCM_CREDENTIALS, &creds(5, 0, 0));
        control.extend(cmsg(SOL_SOCKET, SCM_CREDENTIALS, &creds(6, 0, 0)));
        assert_eq!(parse_control(&control, &mut |_| {}).map(|c| c.pid), Some(6));
    }

    /// The real thing, where there is one: a socketpair, credentials asked
    /// for, a write, and the writer named.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_socketpair_names_its_writer() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;

        let (mut writer, reader) = UnixStream::pair().unwrap();
        pass_credentials(reader.as_raw_fd()).unwrap();
        writer.write_all(b"hello").unwrap();
        let mut buf = [0u8; 16];
        let got = receive(reader.as_raw_fd(), &mut buf).unwrap();
        assert_eq!(&buf[..got.len], b"hello");
        let me = i32::try_from(std::process::id()).unwrap();
        assert_eq!(got.sender.map(|c| c.pid), Some(me));
        assert_eq!(peer_credentials(reader.as_raw_fd()).unwrap().pid, me);
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(pass_credentials(0), Err(ENOSYS));
        assert_eq!(peer_credentials(0), Err(ENOSYS));
        assert_eq!(receive(0, &mut [0u8; 4]), Err(ENOSYS));
    }
}
