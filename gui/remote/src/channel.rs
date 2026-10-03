//! The display protocol over a SlateOS channel: the local transport, whose
//! peer the kernel attests.
//!
//! [`socket`](crate::socket)'s TCP carries the protocol between machines, and
//! until now within one as well. Over TCP the compositor cannot tell who is
//! calling: a TCP peer has no process the kernel will vouch for, so a client's
//! "pid" has been a number the compositor made up per connection, and every
//! shell-only request has gone through a check that cannot check anything.
//! A channel from SlateOS's service registry carries the same frames, and the
//! kernel records who connected: [`ChannelConn::peer_cred`] names the process,
//! and [`ChannelConn::peer_has_key`] says whether it holds the display
//! service's key -- the attestation the compositor's shell check has waited
//! for (lane A's `requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`,
//! point 3, and `design-decisions.md` §1518).
//!
//! ## Messages, not a stream
//!
//! A channel moves whole messages. One `write` is one message, at most the
//! channel's limit (64 KiB, never less, by the kernel's promise). One `read`
//! takes one message, and a buffer too short for it gets its start and loses
//! the rest. The display protocol is a stream of frames that carry their own
//! lengths, so this transport writes a frame as messages of at most the limit
//! ([`send_chunked`]) and reads whole messages into the caller's buffer, where
//! the frames are reassembled exactly as from a TCP read. Every read uses a
//! buffer of the full limit, so no message is ever cut short.
//!
//! ## The kernel calls
//!
//! Five SlateOS extensions to the Linux table, at numbers Linux will not
//! reach, plus `read`, `write`, `poll`, `fstat` and `close`. The decisions this
//! module makes -- the chunking, the read loop, what each errno means -- are
//! written against [`MessagePipe`], so they are tested on the development
//! host against a fake. Only the calls themselves are SlateOS's
//! (`target_vendor = "slateos"`), in the `kernel` section below.

use std::io::{self, ErrorKind};
use std::time::Duration;

/// The service the compositor registers and clients connect to.
pub const DISPLAY_SERVICE: &str = "org.slateos.Display";

/// The smallest message limit a channel has, which the kernel promises never
/// to go below. A descriptor that reports less -- an `fstat` that answers the
/// generic block size, as the first kernel with channel descriptors did -- is
/// read and written at this size.
pub const MIN_MESSAGE_LIMIT: usize = 64 * 1024;

/// The most one [`Transport::read`](crate::client::Transport::read) takes, as
/// [`socket`](crate::socket)'s: a peer writing faster than this process
/// dispatches must not hold the read for ever.
const MAX_READ_PER_CALL: usize = 256 * 1024;

/// How long a write waits on a peer whose queue stays full, as
/// [`socket`](crate::socket)'s: a peer that will not drain is a peer in
/// trouble, and blocking on it for ever would hide that.
const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A Linux error number, as the kernel returns it.
pub type Errno = i32;

/// "Try again": no message waiting, or no room in the peer's queue.
pub const EAGAIN: Errno = 11;
/// A signal interrupted the call before it did anything.
pub const EINTR: Errno = 4;
/// The peer has closed its end.
pub const EPIPE: Errno = 32;
/// No identity recorded for the peer.
pub const ENODATA: Errno = 61;
/// Nothing is registered under the name connected to.
pub const ECONNREFUSED: Errno = 111;
/// The kernel has no such call: one that predates channel descriptors.
pub const ENOSYS: Errno = 38;
/// A message over the channel's limit.
pub const EMSGSIZE: Errno = 90;

/// Who is at the other end of a channel, as the kernel recorded it when the
/// connection was made -- not anything the peer said about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerCred {
    /// The peer's process id.
    pub pid: u32,
    /// Its user id.
    pub uid: u32,
    /// Its group id.
    pub gid: u32,
}

/// One end of a channel, as the logic above the kernel calls sees it.
///
/// Implemented by the real descriptor on SlateOS and by a fake in this
/// module's tests. Everything here is one system call.
pub trait MessagePipe {
    /// Send `message` as one message. Answers its length, or the errno.
    ///
    /// # Errors
    ///
    /// The kernel's errno.
    fn send(&mut self, message: &[u8]) -> Result<usize, Errno>;

    /// Receive one message into `buf`, answering its length; `Ok(0)` when the
    /// peer has closed and nothing is queued.
    ///
    /// # Errors
    ///
    /// The kernel's errno; [`EAGAIN`] when nothing is waiting.
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Errno>;

    /// Wait until the peer's queue has room for a message, or `timeout`
    /// passes. Answers whether there is room.
    ///
    /// # Errors
    ///
    /// The kernel's errno.
    fn wait_writable(&mut self, timeout: Duration) -> Result<bool, Errno>;
}

/// The message limit to use for a descriptor that reports `reported`: the
/// larger of that and [`MIN_MESSAGE_LIMIT`].
#[must_use]
pub fn message_limit(reported: usize) -> usize {
    reported.max(MIN_MESSAGE_LIMIT)
}

/// Send `frame` over `pipe` as messages of at most `limit` bytes, in order.
///
/// A full queue is waited out (up to [`WRITE_STALL_TIMEOUT`] each time): a
/// frame cut in half is worse than no frame, since the peer could never find
/// where the next one starts. Answers `Ok(true)` when every byte went, and
/// `Ok(false)` when the peer closed part-way, which is the ordinary end of a
/// connection rather than a failure -- the bytes are lost, and the caller's
/// loop ends on the connection being closed.
///
/// # Errors
///
/// [`ErrorKind::TimedOut`] for a peer that has not drained for
/// [`WRITE_STALL_TIMEOUT`]; the connection is then unusable, a frame being
/// part-sent. Any other errno, as an [`io::Error`].
pub fn send_chunked<P: MessagePipe>(pipe: &mut P, frame: &[u8], limit: usize) -> io::Result<bool> {
    let limit = message_limit(limit);
    for chunk in frame.chunks(limit) {
        loop {
            match pipe.send(chunk) {
                Ok(_) => break,
                Err(EINTR) => {}
                Err(EAGAIN) => match pipe.wait_writable(WRITE_STALL_TIMEOUT) {
                    Ok(true) | Err(EINTR) => {}
                    Ok(false) => {
                        return Err(io::Error::new(
                            ErrorKind::TimedOut,
                            "the peer has not read for 30 seconds; a frame is part-sent",
                        ));
                    }
                    Err(errno) => return Err(io::Error::from_raw_os_error(errno)),
                },
                Err(EPIPE) => return Ok(false),
                Err(errno) => return Err(io::Error::from_raw_os_error(errno)),
            }
        }
    }
    Ok(true)
}

/// What one round of reading found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Received {
    /// How many bytes were appended.
    pub bytes: usize,
    /// Whether the peer has closed: nothing more will ever arrive.
    pub closed: bool,
}

/// Read whole messages from `pipe` into `out` until none is waiting, the
/// peer closes, or [`MAX_READ_PER_CALL`] bytes have been taken. `scratch` is
/// the per-connection receive buffer, at least the message limit long, so no
/// message is cut short.
///
/// # Errors
///
/// Any errno but [`EAGAIN`], [`EINTR`] and a closed peer, as an
/// [`io::Error`].
pub fn recv_messages<P: MessagePipe>(
    pipe: &mut P,
    scratch: &mut [u8],
    out: &mut Vec<u8>,
) -> io::Result<Received> {
    let mut received = Received {
        bytes: 0,
        closed: false,
    };
    while received.bytes < MAX_READ_PER_CALL {
        match pipe.recv(scratch) {
            Ok(0) => {
                received.closed = true;
                break;
            }
            Ok(n) => {
                // A kernel answering more than the buffer is not trusted to
                // have written it: the bytes past the buffer are not there.
                let Some(message) = scratch.get(..n) else {
                    break;
                };
                out.extend_from_slice(message);
                received.bytes = received.bytes.saturating_add(n);
            }
            Err(EAGAIN) => break,
            Err(EINTR) => {}
            Err(EPIPE) => {
                received.closed = true;
                break;
            }
            Err(errno) => return Err(io::Error::from_raw_os_error(errno)),
        }
    }
    Ok(received)
}

/// What a failed connect means to a caller choosing a transport.
///
/// `true` for the two answers that mean "no display here by this route" --
/// nothing registered under the name ([`ECONNREFUSED`]) or a kernel without
/// channel descriptors at all ([`ENOSYS`]) -- after which a client may try
/// TCP. Anything else (out of descriptors, a fault) is a real failure, and
/// falling back would hide it.
#[must_use]
pub fn connect_failure_means_absent(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(ECONNREFUSED | ENOSYS))
}

#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
pub use kernel::{ChannelConn, ChannelListener};

/// The real descriptors and the system calls on them: SlateOS only.
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
mod kernel {
    use std::arch::asm;
    use std::io;
    use std::os::fd::RawFd;
    use std::sync::Arc;
    use std::task::Waker;
    use std::time::Duration;

    use super::{
        ENODATA, Errno, MessagePipe, PeerCred, Received, message_limit, recv_messages, send_chunked,
    };
    use crate::client::Transport;
    use crate::wait::{AsWaitHandle, WaitHandle, WaitSet, WakeReceiver, WakeSender, wake_channel};

    const SYS_READ: u64 = 0;
    const SYS_WRITE: u64 = 1;
    const SYS_CLOSE: u64 = 3;
    const SYS_FSTAT: u64 = 5;
    const SYS_POLL: u64 = 7;
    const SLATE_SERVICE_REGISTER: u64 = 1001;
    const SLATE_SERVICE_ACCEPT: u64 = 1002;
    const SLATE_SERVICE_CONNECT: u64 = 1003;
    const SLATE_CHANNEL_PEER_CRED: u64 = 1004;
    const SLATE_CHANNEL_PEER_HAS_KEY: u64 = 1005;

    /// Every descriptor made here: non-blocking, because a compositor has a
    /// frame to draw whether or not a client has spoken; close-on-exec,
    /// because a program's connection to the display must not leak into a
    /// child it starts.
    const FLAGS: u64 = 0o4000 | 0o2_000_000;
    /// `POLLOUT`.
    const POLLOUT: i16 = 0x004;
    /// Where `st_blksize` sits in the x86-64 Linux `struct stat` (144 bytes).
    const STAT_BLKSIZE_AT: usize = 56;
    const STAT_SIZE: usize = 144;

    /// Issue a system call with up to three arguments.
    ///
    /// # Safety
    ///
    /// The arguments must be valid for the call named by `n`: any pointer must
    /// point to memory of the size the kernel will read or write.
    unsafe fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> i64 {
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, declared
        // below, and returns in `rax`; the argument registers are the x86-64
        // Linux syscall ABI, which SlateOS's Linux table follows. Validity of
        // the arguments is this function's documented precondition.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") n as i64 => ret,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }

    /// A raw return as a result: `-4095..0` is `-errno`.
    fn decode(ret: i64) -> Result<i64, Errno> {
        if (-4095..0).contains(&ret) {
            Err(ret
                .checked_neg()
                .and_then(|v| Errno::try_from(v).ok())
                .unwrap_or(super::ENOSYS))
        } else {
            Ok(ret)
        }
    }

    /// A descriptor from a successful call.
    fn as_fd(ret: Result<i64, Errno>) -> Result<RawFd, Errno> {
        ret.and_then(|fd| RawFd::try_from(fd).map_err(|_| super::ENOSYS))
    }

    fn close(fd: RawFd) {
        // SAFETY: closing takes no pointer; a descriptor this module owns.
        // The result is dropped: there is nothing to do about a failed close,
        // and the descriptor is gone either way.
        let _ = unsafe { syscall3(SYS_CLOSE, u64::from(fd.unsigned_abs()), 0, 0) };
    }

    /// The descriptor's message limit, from `fstat`'s `st_blksize`.
    fn reported_limit(fd: RawFd) -> usize {
        let mut stat = [0u8; STAT_SIZE];
        // SAFETY: `stat` is `STAT_SIZE` writable bytes, the size of the x86-64
        // `struct stat` the kernel writes.
        let ret = unsafe {
            syscall3(
                SYS_FSTAT,
                u64::from(fd.unsigned_abs()),
                stat.as_mut_ptr() as u64,
                0,
            )
        };
        if decode(ret).is_err() {
            return 0;
        }
        stat.get(STAT_BLKSIZE_AT..STAT_BLKSIZE_AT + 8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map(i64::from_ne_bytes)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    }

    /// A descriptor's message pipe: the system calls behind [`MessagePipe`].
    struct Fd(RawFd);

    impl MessagePipe for Fd {
        fn send(&mut self, message: &[u8]) -> Result<usize, Errno> {
            // SAFETY: `message` is `message.len()` readable bytes.
            let ret = unsafe {
                syscall3(
                    SYS_WRITE,
                    u64::from(self.0.unsigned_abs()),
                    message.as_ptr() as u64,
                    message.len() as u64,
                )
            };
            decode(ret).and_then(|n| usize::try_from(n).map_err(|_| super::ENOSYS))
        }

        fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
            // SAFETY: `buf` is `buf.len()` writable bytes.
            let ret = unsafe {
                syscall3(
                    SYS_READ,
                    u64::from(self.0.unsigned_abs()),
                    buf.as_mut_ptr() as u64,
                    buf.len() as u64,
                )
            };
            decode(ret).and_then(|n| usize::try_from(n).map_err(|_| super::ENOSYS))
        }

        fn wait_writable(&mut self, timeout: Duration) -> Result<bool, Errno> {
            // `struct pollfd { int fd; short events; short revents; }`.
            let mut pollfd = [0u8; 8];
            pollfd[..4].copy_from_slice(&self.0.to_ne_bytes());
            pollfd[4..6].copy_from_slice(&POLLOUT.to_ne_bytes());
            let ms = u64::try_from(timeout.as_millis())
                .unwrap_or(u64::MAX)
                .min(i32::MAX as u64);
            // SAFETY: `pollfd` is one 8-byte `struct pollfd`, read and written.
            let ret = unsafe { syscall3(SYS_POLL, pollfd.as_mut_ptr() as u64, 1, ms) };
            decode(ret).map(|ready| ready > 0)
        }
    }

    /// A program's connection to a service over a channel, or the service's
    /// end of one: a display-protocol [`Transport`] whose peer the kernel
    /// attests.
    pub struct ChannelConn {
        fd: Fd,
        /// The largest message the channel carries.
        limit: usize,
        /// The receive buffer, `limit` long, reused for every message.
        scratch: Vec<u8>,
        /// Sticky, as [`Socket`](crate::socket::Socket)'s: set once the peer
        /// has closed.
        open: bool,
        /// How long [`Transport::wait`] parks.
        wait_timeout: Option<Duration>,
        /// Made the first time [`Transport::waker`] is asked for.
        wake: Option<(WakeReceiver, Arc<WakeSender>)>,
        /// The set [`Transport::wait`] parks on, reused.
        waits: WaitSet,
    }

    impl ChannelConn {
        fn adopt(fd: RawFd) -> Self {
            let limit = message_limit(reported_limit(fd));
            Self {
                fd: Fd(fd),
                limit,
                scratch: vec![0u8; limit],
                open: true,
                wait_timeout: None,
                wake: None,
                waits: WaitSet::new(),
            }
        }

        /// Connect to the service registered as `name`.
        ///
        /// # Errors
        ///
        /// `ECONNREFUSED` if nothing is registered under the name, `ENOSYS` on
        /// a kernel without channel descriptors
        /// ([`connect_failure_means_absent`](super::connect_failure_means_absent)
        /// says which a caller may fall back from), or the kernel's other
        /// errnos.
        pub fn connect(name: &str) -> io::Result<Self> {
            // SAFETY: `name` is `name.len()` readable bytes; the kernel copies
            // it in and keeps no pointer.
            let ret = unsafe {
                syscall3(
                    SLATE_SERVICE_CONNECT,
                    name.as_ptr() as u64,
                    name.len() as u64,
                    FLAGS,
                )
            };
            as_fd(decode(ret))
                .map(Self::adopt)
                .map_err(io::Error::from_raw_os_error)
        }

        /// Who is at the other end, as the kernel recorded it at connect time;
        /// `None` when it recorded nobody.
        ///
        /// # Errors
        ///
        /// The kernel's errno, other than "nobody recorded".
        pub fn peer_cred(&self) -> io::Result<Option<PeerCred>> {
            let mut out = [0u8; 12];
            // SAFETY: `out` is the 12 writable bytes the call writes.
            let ret = unsafe {
                syscall3(
                    SLATE_CHANNEL_PEER_CRED,
                    u64::from(self.fd.0.unsigned_abs()),
                    out.as_mut_ptr() as u64,
                    0,
                )
            };
            match decode(ret) {
                Ok(_) => {
                    let word = |at: usize| {
                        out.get(at..at.saturating_add(4))
                            .and_then(|b| <[u8; 4]>::try_from(b).ok())
                            .map_or(0, u32::from_ne_bytes)
                    };
                    Ok(Some(PeerCred {
                        pid: word(0),
                        uid: word(4),
                        gid: word(8),
                    }))
                }
                Err(ENODATA) => Ok(None),
                Err(errno) => Err(io::Error::from_raw_os_error(errno)),
            }
        }

        /// Whether the process at the other end holds the key of the service
        /// it connected to -- for the display service, whether it is the
        /// shell. `None` when the kernel has no identity to judge.
        ///
        /// # Errors
        ///
        /// The kernel's errno, other than "no identity".
        pub fn peer_has_key(&self) -> io::Result<Option<bool>> {
            // SAFETY: the call takes no pointer.
            let ret = unsafe {
                syscall3(
                    SLATE_CHANNEL_PEER_HAS_KEY,
                    u64::from(self.fd.0.unsigned_abs()),
                    0,
                    0,
                )
            };
            match decode(ret) {
                Ok(holds) => Ok(Some(holds != 0)),
                Err(ENODATA) => Ok(None),
                Err(errno) => Err(io::Error::from_raw_os_error(errno)),
            }
        }

        /// Hang up.
        pub fn close(&mut self) {
            self.open = false;
        }
    }

    impl Drop for ChannelConn {
        fn drop(&mut self) {
            close(self.fd.0);
        }
    }

    impl Transport for ChannelConn {
        type Error = io::Error;

        fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
            if !self.open {
                return Ok(0);
            }
            let Received { bytes, closed } = recv_messages(&mut self.fd, &mut self.scratch, buf)?;
            if closed {
                self.open = false;
            }
            Ok(bytes)
        }

        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            if !self.open || bytes.is_empty() {
                return Ok(());
            }
            match send_chunked(&mut self.fd, bytes, self.limit) {
                Ok(true) => Ok(()),
                // The peer is gone: not a failure, as for a TCP peer.
                Ok(false) => {
                    self.open = false;
                    Ok(())
                }
                // A frame part-sent can never be parsed again.
                Err(e) => {
                    self.open = false;
                    Err(e)
                }
            }
        }

        fn is_open(&self) -> bool {
            self.open
        }

        fn wait(&mut self) -> io::Result<()> {
            if !self.open {
                return Ok(());
            }
            self.waits.clear();
            self.waits.add(self.fd.0);
            let woken = self
                .wake
                .as_ref()
                .map(|(receiver, _)| self.waits.add_source(receiver));
            self.waits.wait(self.wait_timeout)?;
            if let (Some(index), Some((receiver, _))) = (woken, self.wake.as_mut())
                && self.waits.is_ready(index)
            {
                receiver.drain();
            }
            Ok(())
        }

        fn set_wait_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
            if timeout.is_some_and(|d| d.is_zero()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a zero wait timeout means 'never time out', which is not what a caller means",
                ));
            }
            self.wait_timeout = timeout;
            Ok(())
        }

        fn waker(&mut self) -> io::Result<Option<Waker>> {
            if self.wake.is_none() {
                let (sender, receiver) = wake_channel()?;
                self.wake = Some((receiver, Arc::new(sender)));
            }
            Ok(self
                .wake
                .as_ref()
                .map(|(_, sender)| Waker::from(Arc::clone(sender))))
        }
    }

    impl AsWaitHandle for ChannelConn {
        /// The channel descriptor: readable when a message waits or the peer
        /// has closed.
        fn wait_handle(&self) -> WaitHandle {
            self.fd.0
        }
    }

    /// A service's listening end: accepts [`ChannelConn`]s.
    pub struct ChannelListener {
        fd: RawFd,
    }

    impl ChannelListener {
        /// Register the service `name` and listen on it.
        ///
        /// # Errors
        ///
        /// `EACCES` without the `Service` capability with `WRITE`,
        /// `EADDRINUSE` if the name is taken, or the kernel's other errnos.
        pub fn register(name: &str) -> io::Result<Self> {
            // SAFETY: `name` is `name.len()` readable bytes; copied in.
            let ret = unsafe {
                syscall3(
                    SLATE_SERVICE_REGISTER,
                    name.as_ptr() as u64,
                    name.len() as u64,
                    FLAGS,
                )
            };
            as_fd(decode(ret))
                .map(|fd| Self { fd })
                .map_err(io::Error::from_raw_os_error)
        }

        /// The next client, or `None` if none is waiting.
        ///
        /// # Errors
        ///
        /// The kernel's errno, other than "nobody waiting".
        pub fn accept(&self) -> io::Result<Option<ChannelConn>> {
            // SAFETY: the call takes no pointer.
            let ret = unsafe {
                syscall3(
                    SLATE_SERVICE_ACCEPT,
                    u64::from(self.fd.unsigned_abs()),
                    FLAGS,
                    0,
                )
            };
            match as_fd(decode(ret)) {
                Ok(fd) => Ok(Some(ChannelConn::adopt(fd))),
                Err(super::EAGAIN) => Ok(None),
                Err(errno) => Err(io::Error::from_raw_os_error(errno)),
            }
        }
    }

    impl Drop for ChannelListener {
        fn drop(&mut self) {
            close(self.fd);
        }
    }

    impl AsWaitHandle for ChannelListener {
        /// Readable when a client is waiting to be accepted.
        fn wait_handle(&self) -> WaitHandle {
            self.fd
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::collections::VecDeque;

    use super::*;

    /// A channel end with a scripted peer: what it queues for this side to
    /// read, how many messages the peer's queue holds before it is full, and
    /// what this side sent.
    #[derive(Default)]
    struct FakePipe {
        incoming: VecDeque<Result<Vec<u8>, Errno>>,
        sent: Vec<Vec<u8>>,
        /// Answers `send` gives before taking messages, in order.
        send_errors: VecDeque<Errno>,
        /// Answers `wait_writable` gives, in order; `true` once exhausted.
        writable: VecDeque<Result<bool, Errno>>,
        limit: usize,
    }

    impl MessagePipe for FakePipe {
        fn send(&mut self, message: &[u8]) -> Result<usize, Errno> {
            if message.len() > self.limit {
                return Err(EMSGSIZE);
            }
            if let Some(errno) = self.send_errors.pop_front() {
                return Err(errno);
            }
            self.sent.push(message.to_vec());
            Ok(message.len())
        }

        fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
            match self.incoming.pop_front() {
                None => Err(EAGAIN),
                Some(Err(errno)) => Err(errno),
                Some(Ok(message)) => {
                    // A short buffer gets the start, as the kernel's does.
                    let n = message.len().min(buf.len());
                    buf[..n].copy_from_slice(&message[..n]);
                    Ok(n)
                }
            }
        }

        fn wait_writable(&mut self, _timeout: Duration) -> Result<bool, Errno> {
            self.writable.pop_front().unwrap_or(Ok(true))
        }
    }

    fn pipe(limit: usize) -> FakePipe {
        FakePipe {
            limit,
            ..FakePipe::default()
        }
    }

    #[test]
    fn a_frame_larger_than_the_limit_goes_as_messages_of_the_limit_in_order() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        let frame: Vec<u8> = (0..MIN_MESSAGE_LIMIT * 2 + 10)
            .map(|i| (i % 251) as u8)
            .collect();
        assert!(send_chunked(&mut p, &frame, MIN_MESSAGE_LIMIT).unwrap());
        assert_eq!(
            p.sent.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![MIN_MESSAGE_LIMIT, MIN_MESSAGE_LIMIT, 10]
        );
        assert_eq!(p.sent.concat(), frame);
    }

    #[test]
    fn a_reported_limit_below_the_promised_floor_is_not_believed() {
        // The first kernel's fstat answered the generic 4096; writing at that
        // size would be sixteen times the messages for nothing.
        assert_eq!(message_limit(4096), MIN_MESSAGE_LIMIT);
        assert_eq!(message_limit(0), MIN_MESSAGE_LIMIT);
        assert_eq!(message_limit(1 << 20), 1 << 20);
    }

    #[test]
    fn a_full_queue_is_waited_out_and_the_frame_finished() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        p.send_errors = VecDeque::from([EAGAIN, EINTR, EAGAIN]);
        let frame = vec![7u8; 100];
        assert!(send_chunked(&mut p, &frame, MIN_MESSAGE_LIMIT).unwrap());
        assert_eq!(p.sent, vec![frame]);
    }

    #[test]
    fn a_peer_that_never_drains_is_an_error_not_a_hang() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        p.send_errors = VecDeque::from([EAGAIN]);
        p.writable = VecDeque::from([Ok(false)]);
        let err = send_chunked(&mut p, &[1, 2, 3], MIN_MESSAGE_LIMIT).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn a_peer_that_closed_mid_frame_ends_the_connection_quietly() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        p.send_errors = VecDeque::from([EPIPE]);
        assert!(!send_chunked(&mut p, &[1, 2, 3], MIN_MESSAGE_LIMIT).unwrap());
    }

    #[test]
    fn messages_are_read_whole_and_in_order_until_none_waits() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        p.incoming = VecDeque::from([Ok(vec![1, 2, 3]), Err(EINTR), Ok(vec![4, 5])]);
        let mut scratch = vec![0u8; MIN_MESSAGE_LIMIT];
        let mut out = Vec::new();
        let got = recv_messages(&mut p, &mut scratch, &mut out).unwrap();
        assert_eq!(
            got,
            Received {
                bytes: 5,
                closed: false
            }
        );
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_closed_peer_is_reported_after_what_it_queued() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        p.incoming = VecDeque::from([Ok(vec![9]), Ok(Vec::new())]);
        let mut scratch = vec![0u8; MIN_MESSAGE_LIMIT];
        let mut out = Vec::new();
        let got = recv_messages(&mut p, &mut scratch, &mut out).unwrap();
        assert!(got.closed);
        assert_eq!(out, vec![9]);
    }

    #[test]
    fn a_read_stops_at_its_budget_and_leaves_the_rest_queued() {
        let mut p = pipe(MIN_MESSAGE_LIMIT);
        let message = vec![0u8; MIN_MESSAGE_LIMIT];
        for _ in 0..(MAX_READ_PER_CALL / MIN_MESSAGE_LIMIT + 2) {
            p.incoming.push_back(Ok(message.clone()));
        }
        let mut scratch = vec![0u8; MIN_MESSAGE_LIMIT];
        let mut out = Vec::new();
        let got = recv_messages(&mut p, &mut scratch, &mut out).unwrap();
        assert_eq!(got.bytes, MAX_READ_PER_CALL);
        assert_eq!(p.incoming.len(), 2, "the read took more than its budget");
    }

    #[test]
    fn frames_survive_being_cut_into_messages_and_read_back() {
        // The whole point: the protocol's frames, cut at the message limit by
        // one side, reassemble on the other into exactly the bytes sent.
        let mut writer = pipe(MIN_MESSAGE_LIMIT);
        let frame: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 256) as u8).collect();
        assert!(send_chunked(&mut writer, &frame, MIN_MESSAGE_LIMIT).unwrap());
        let mut reader = pipe(MIN_MESSAGE_LIMIT);
        reader.incoming = writer.sent.into_iter().map(Ok).collect();
        let mut scratch = vec![0u8; MIN_MESSAGE_LIMIT];
        let mut out = Vec::new();
        while !reader.incoming.is_empty() {
            recv_messages(&mut reader, &mut scratch, &mut out).unwrap();
        }
        assert_eq!(out, frame);
    }

    #[test]
    fn only_a_missing_service_or_kernel_call_is_a_reason_to_fall_back() {
        assert!(connect_failure_means_absent(&io::Error::from_raw_os_error(
            ECONNREFUSED
        )));
        assert!(connect_failure_means_absent(&io::Error::from_raw_os_error(
            ENOSYS
        )));
        assert!(!connect_failure_means_absent(
            &io::Error::from_raw_os_error(24)
        ));
    }
}
