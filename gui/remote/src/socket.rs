//! The transport that crosses a process boundary.
//!
//! [`loopback`](crate::loopback) proved the codecs against each other inside
//! one process; it cannot carry a frame between two. This module can. A
//! [`Socket`] is a connection to the compositor that implements [`Transport`],
//! and a [`Listener`] is the compositor's end: it accepts them.
//!
//! A socket runs over one of two carriers: TCP, everywhere, and on SlateOS a
//! channel from the kernel's service registry ([`channel`](crate::channel)),
//! whose peer the kernel names. Nothing above [`Transport`] knows which, and
//! that is the reason the trait is where it is.
//!
//! ## Why TCP, and why that is not a stopgap
//!
//! This crate is, by its own first line, a *remote*-desktop protocol. A
//! transport that only worked between two processes on one machine would
//! contradict the thing the protocol exists for. TCP is also the one carrier
//! that behaves identically on the hosted development build and on SlateOS
//! itself, so the same client code is exercised in both places rather than one
//! path being tested and the other merely written.
//!
//! ## Why a channel as well
//!
//! Not for speed. On one machine TCP costs a loopback round trip instead of a
//! kernel channel, measured in single-digit microseconds and far below a frame
//! budget. The reason is that a TCP peer cannot be asked what process it is,
//! and a channel's can: the kernel records who connected
//! ([`Socket::peer_cred`]) and whether they hold the service's key
//! ([`Socket::peer_has_key`]). That is what lets the compositor say which
//! program owns a window, and tell the shell from an application by something
//! other than what the application claims.
//!
//! ## Where the compositor is
//!
//! [`SLATE_DISPLAY`](DISPLAY_VAR) says: a TCP address, or `service:NAME` for a
//! SlateOS service ([`SERVICE_PREFIX`]) -- the same arrangement as X11's
//! `DISPLAY`, for the same reason: an application must not have the address of
//! its display server compiled into it. Unset, it is the default display: on
//! SlateOS the display service
//! ([`DISPLAY_SERVICE`](crate::channel::DISPLAY_SERVICE)), and [`DEFAULT_DISPLAY`]
//! over TCP for a client only when no compositor serves that; elsewhere
//! [`DEFAULT_DISPLAY`]. [`Socket::connect_display`] and
//! [`Listener::bind_display`] apply those rules from the two ends.
//!
//! ## Blocking discipline
//!
//! [`Transport::read`] must not block and [`Transport::wait`] must. A socket
//! cannot be both at once, so a [`Socket`] is held in non-blocking mode and
//! `wait` briefly switches it back, [`peek`](TcpStream::peek)s one byte — which
//! blocks until a byte is *available* without consuming it — and switches
//! forward again. The alternative, polling on a timer, would either add latency
//! to every keystroke or wake an idle desktop hundreds of times a second.
//!
//! A socket that has handed out a [`Transport::waker`] waits differently,
//! because it has two things to wait on: the stream, and the pipe another
//! thread writes to wake it. Those go into a [`WaitSet`] together. The
//! `peek` path stays for every socket that never asks for a waker — which is
//! nearly all of them — since a lone blocking read is the one wait a platform
//! can always carry out without polling.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::task::Waker;
use std::time::Duration;

use crate::client::Transport;
use crate::wait::{AsWaitHandle, WaitHandle, WaitSet, WakeReceiver, WakeSender, wake_channel};

/// The environment variable naming the compositor's address.
pub const DISPLAY_VAR: &str = "SLATE_DISPLAY";

/// Where the compositor listens when `SLATE_DISPLAY` says nothing.
///
/// Loopback rather than `0.0.0.0`: a display server that accepts connections
/// from the network by default would hand every machine on the LAN the
/// keystrokes of every window on this one. Remote display is a thing to opt
/// into with an address, not a thing to discover you had.
pub const DEFAULT_DISPLAY: &str = "127.0.0.1:7373";

/// How much one [`Transport::read`] will take before returning.
///
/// A read that drained the socket completely could be held there indefinitely
/// by a peer writing faster than this process dispatches, and the events
/// already read would never be acted on. Returning early costs nothing: the
/// remainder stays in the kernel's buffer, where [`Socket::wait`] sees it
/// immediately and the next read collects it.
const MAX_READ_PER_CALL: usize = 256 * 1024;

/// Scratch size for one `recv`. Large enough that a burst of input frames or a
/// whole redraw arrives in one or two syscalls, small enough to sit on the
/// stack.
const CHUNK: usize = 8 * 1024;

/// What [`SLATE_DISPLAY`](DISPLAY_VAR) says, or [`DEFAULT_DISPLAY`] when it is
/// unset.
///
/// Only the TCP half of the default: on SlateOS an unset variable means the
/// display service first ([`Socket::connect_display`],
/// [`Listener::bind_display`]), which those apply themselves. This is the
/// thing to name in a message about where a program looked.
///
/// # Errors
///
/// [`ErrorKind::InvalidInput`] if `SLATE_DISPLAY` is set to something that is
/// not UTF-8. Reported rather than ignored: an address the user deliberately
/// set and that this process cannot read is a configuration error, and silently
/// falling back to the default would connect to the wrong display and look like
/// the variable had no effect.
pub fn display_addr() -> io::Result<String> {
    match std::env::var_os(DISPLAY_VAR) {
        None => Ok(DEFAULT_DISPLAY.to_string()),
        Some(raw) => raw.into_string().map_err(|bad| {
            io::Error::new(
                ErrorKind::InvalidInput,
                // `display` substitutes replacement characters for the bytes it
                // cannot read. Lossy is right *here* and nowhere else: this
                // string is a diagnostic a person will read, not data anything
                // will act on, and quoting the setting back is what makes the
                // mistake visible.
                format!("{DISPLAY_VAR} is not valid UTF-8: {}", bad.display()),
            )
        }),
    }
}

/// Whether an error means the peer went away rather than that something broke.
///
/// A compositor shutting down, or a client's process exiting, is the ordinary
/// end of a connection and reaches the reader as one of these. Treating them as
/// failures would make every clean exit print a transport error; treating a
/// genuine failure as a hang-up would hide it. The list is exactly the kinds
/// that mean "this connection is over".
fn is_hangup(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof
    )
}

/// A connected transport to the compositor.
///
/// Named for what it is to its user — the socket the display protocol runs over
/// — rather than for the carrier underneath: TCP everywhere, and on SlateOS a
/// channel from the service registry ([`channel`](crate::channel)), whose peer
/// the kernel attests. An application that says `socket::connect_display()`
/// names neither.
pub struct Socket {
    carrier: Carrier,
}

/// What a [`Socket`] runs over.
enum Carrier {
    /// A TCP stream: between machines, and on any host.
    Tcp(Tcp),
    /// A SlateOS channel: a local connection whose peer the kernel names.
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    Channel(crate::channel::ChannelConn),
}

/// The TCP carrier.
struct Tcp {
    stream: TcpStream,
    /// Sticky: set once the peer hangs up, so [`Transport::is_open`] keeps
    /// answering `false` without another syscall, and so a hang-up noticed
    /// during a `read` is still visible to a caller that checks afterwards.
    open: bool,
    /// How long [`Transport::wait`] will park before returning with nothing.
    /// `None` — the default — parks until something actually happens, which is
    /// what an event-driven application wants and what keeps an idle desktop
    /// genuinely idle.
    wait_timeout: Option<Duration>,
    /// Made the first time [`Transport::waker`] is asked for, and not before:
    /// most applications never wake their loop from another thread, and need
    /// not pay a pipe for it.
    wake: Option<Wake>,
}

/// What a [`Socket`] needs to be woken from another thread: the waited-on
/// half of a wake channel, the half handed out, and the set the two are waited
/// on in together.
#[derive(Debug)]
struct Wake {
    receiver: WakeReceiver,
    sender: Arc<WakeSender>,
    set: WaitSet,
}

impl Socket {
    /// Dial `addr` over TCP.
    ///
    /// # Errors
    ///
    /// Whatever the connection attempt fails with; also
    /// [`ErrorKind::InvalidInput`] if `addr` resolves to nothing.
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        Self::adopt(TcpStream::connect(addr)?)
    }

    /// Dial the display [`SLATE_DISPLAY`](DISPLAY_VAR) names: a TCP address,
    /// or `service:NAME` for a SlateOS service.
    ///
    /// Unset, the display is the local compositor's service
    /// ([`crate::channel::DISPLAY_SERVICE`]) on SlateOS, falling back to
    /// [`DEFAULT_DISPLAY`] over TCP only when no such service exists -- a
    /// compositor not registered as one, or a kernel without channel
    /// descriptors. Elsewhere it is [`DEFAULT_DISPLAY`].
    ///
    /// # Errors
    ///
    /// As [`Self::connect`], plus the environment error [`display_addr`]
    /// reports, and [`ErrorKind::Unsupported`] for a service named on a
    /// platform without them.
    pub fn connect_display() -> io::Result<Self> {
        match std::env::var_os(DISPLAY_VAR) {
            Some(_) => Self::connect_to_display(&display_addr()?),
            None => Self::connect_default(),
        }
    }

    /// Dial `display`, written as [`SLATE_DISPLAY`](DISPLAY_VAR) is: a TCP
    /// address, or `service:NAME`.
    ///
    /// # Errors
    ///
    /// As [`Self::connect_display`], minus the environment.
    pub fn connect_to_display(display: &str) -> io::Result<Self> {
        match display.strip_prefix(SERVICE_PREFIX) {
            Some(name) => Self::connect_service(name),
            None => Self::connect(display),
        }
    }

    /// The display a program reaches when nothing names one.
    fn connect_default() -> io::Result<Self> {
        #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
        match Self::connect_service(crate::channel::DISPLAY_SERVICE) {
            Ok(socket) => return Ok(socket),
            Err(e) if crate::channel::connect_failure_means_absent(&e) => {}
            Err(e) => return Err(e),
        }
        Self::connect(DEFAULT_DISPLAY)
    }

    /// Connect to the SlateOS service `name`.
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    fn connect_service(name: &str) -> io::Result<Self> {
        Ok(Self {
            carrier: Carrier::Channel(crate::channel::ChannelConn::connect(name)?),
        })
    }

    /// Services are SlateOS's; anywhere else, naming one is a mistake to
    /// report, not an address to try.
    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    fn connect_service(name: &str) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            format!("{DISPLAY_VAR} names the service {name:?}, and only SlateOS has services"),
        ))
    }

    /// Take over an already-connected stream — the listener's side.
    ///
    /// # Errors
    ///
    /// If the stream cannot be put into the mode this transport requires.
    pub fn adopt(stream: TcpStream) -> io::Result<Self> {
        Ok(Self {
            carrier: Carrier::Tcp(Tcp::adopt(stream)?),
        })
    }

    /// The peer's address: a TCP peer's.
    ///
    /// # Errors
    ///
    /// If the socket has no peer -- it has been shut down -- or is a channel,
    /// whose peer is named by [`Self::peer_cred`] instead.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match &self.carrier {
            Carrier::Tcp(tcp) => tcp.stream.peer_addr(),
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(_) => Err(io::Error::new(
                ErrorKind::Unsupported,
                "a channel's peer is a process, not an address",
            )),
        }
    }

    /// The local address: a TCP socket's.
    ///
    /// # Errors
    ///
    /// If the socket is not bound, or is a channel.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.carrier {
            Carrier::Tcp(tcp) => tcp.stream.local_addr(),
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(_) => Err(io::Error::new(
                ErrorKind::Unsupported,
                "a channel has no address",
            )),
        }
    }

    /// Whether this runs over a SlateOS channel rather than TCP.
    #[must_use]
    pub const fn is_channel(&self) -> bool {
        match self.carrier {
            Carrier::Tcp(_) => false,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(_) => true,
        }
    }

    /// Who is at the other end, as the kernel recorded it: a channel's peer
    /// process. `None` over TCP, whose peer the kernel cannot vouch for, and
    /// for a channel whose peer it recorded nothing about.
    #[must_use]
    pub fn peer_cred(&self) -> Option<crate::channel::PeerCred> {
        match &self.carrier {
            Carrier::Tcp(_) => None,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            // An error asking is as good as no answer: the identity is either
            // attested or not known.
            Carrier::Channel(conn) => conn.peer_cred().ok().flatten(),
        }
    }

    /// Whether the peer holds the key of the service it connected to -- for
    /// the display service, whether it is the shell. `None` over TCP and when
    /// the kernel cannot say.
    #[must_use]
    pub fn peer_has_key(&self) -> Option<bool> {
        match &self.carrier {
            Carrier::Tcp(_) => None,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(conn) => conn.peer_has_key().ok().flatten(),
        }
    }

    /// Hang up, so both this side and the peer see the connection end.
    pub fn close(&mut self) {
        match &mut self.carrier {
            Carrier::Tcp(tcp) => tcp.close(),
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(conn) => conn.close(),
        }
    }
}

/// Dispatch one [`Transport`] call to the carrier.
macro_rules! carried {
    ($self:ident, $carrier:ident => $call:expr) => {
        match &mut $self.carrier {
            Carrier::Tcp($carrier) => $call,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel($carrier) => $call,
        }
    };
}

impl Transport for Socket {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        carried!(self, c => c.read(buf))
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        carried!(self, c => c.write(bytes))
    }

    fn is_open(&self) -> bool {
        match &self.carrier {
            Carrier::Tcp(tcp) => tcp.is_open(),
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(conn) => conn.is_open(),
        }
    }

    fn wait(&mut self) -> io::Result<()> {
        carried!(self, c => c.wait())
    }

    fn set_wait_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        carried!(self, c => c.set_wait_timeout(timeout))
    }

    fn waker(&mut self) -> io::Result<Option<Waker>> {
        carried!(self, c => c.waker())
    }
}

impl AsWaitHandle for Socket {
    /// The carrier underneath, so a server can wait on many of these at once
    /// ([`WaitSet`]) rather than parking on one with [`Transport::wait`].
    fn wait_handle(&self) -> WaitHandle {
        match &self.carrier {
            Carrier::Tcp(tcp) => tcp.stream.wait_handle(),
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            Carrier::Channel(conn) => conn.wait_handle(),
        }
    }
}

/// How [`SLATE_DISPLAY`](DISPLAY_VAR) names a SlateOS service rather than a
/// TCP address: `service:org.slateos.Display`.
pub const SERVICE_PREFIX: &str = "service:";

impl Tcp {
    fn adopt(stream: TcpStream) -> io::Result<Self> {
        // Nagle's algorithm holds a small write back for up to 40 ms hoping to
        // coalesce it with the next one. Every frame here is small and latency
        // is the whole point: a keystroke's echo must not wait for a second
        // keystroke to give it company.
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            open: true,
            wait_timeout: None,
            wake: None,
        })
    }

    /// Hang up, so both this side and the peer see the connection end.
    fn close(&mut self) {
        self.open = false;
        // The peer learns of this from its own read returning zero. A failure
        // here means the socket was already down, which is the state we are
        // asking for.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    /// The blocking half of [`Transport::wait`], with the socket already
    /// switched to blocking mode. Split out so the caller can restore the mode
    /// on every exit path without a closure borrowing `self` twice.
    fn park(&mut self) -> io::Result<()> {
        self.stream.set_read_timeout(self.wait_timeout)?;
        let mut probe = [0u8; 1];
        match self.stream.peek(&mut probe) {
            // A byte is there — or the peer hung up, which is equally something
            // to wake up for: the caller's next read turns it into a closed
            // connection.
            Ok(0) => {
                self.open = false;
                Ok(())
            }
            Ok(_) => Ok(()),
            // The timeout expired, or a signal cut the wait short. Neither is a
            // failure: `wait` promises only that it may return, not that
            // anything arrived, and every caller re-checks by reading.
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                Ok(())
            }
            Err(e) if is_hangup(e.kind()) => {
                self.open = false;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

/// How many bytes the next chunk read may take, given how many this call has
/// taken already.
///
/// Clamping here rather than checking after the read is what makes
/// [`MAX_READ_PER_CALL`] an actual cap. The loop's guard is tested *before* its
/// body, so a body that always reads a whole `CHUNK` has the postcondition
/// `total < MAX_READ_PER_CALL + CHUNK` instead — it lands on the cap exactly
/// only while every read is full-length. That holds on an idle machine, where
/// `total` walks the `CHUNK` grid and `MAX_READ_PER_CALL` is exactly `32 *
/// CHUNK`, and stops holding the moment the peer's writer is descheduled
/// mid-stream: one short read takes `total` off the grid and the final
/// iteration straddles the boundary.
///
/// Nothing is lost by the shorter read. The remainder stays in the kernel
/// buffer, which is what `MAX_READ_PER_CALL`'s own doc comment promises
/// happens to everything past the cap, and [`Socket::wait`] reports it as
/// readable immediately.
///
/// **This is a free function because the bug was invisible to every test that
/// could be written against the socket.** Provoking it needs a short read to
/// land mid-loop, which depends on when the OS deschedules the writer thread;
/// lane B hit it in ordinary workspace runs and then failed to reproduce it in
/// 128 deliberate attempts. As a function of `total` alone the property is
/// exhaustively checkable, and
/// `the_read_budget_never_lets_a_chunk_cross_the_cap` checks it. See
/// `requests/b-c-guiremote-read-can-overshoot-its-own-cap-by-one-chunk.md`.
const fn read_budget(total: usize) -> usize {
    let remaining = MAX_READ_PER_CALL.saturating_sub(total);
    if remaining < CHUNK { remaining } else { CHUNK }
}

impl Transport for Tcp {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let mut total = 0usize;
        let mut chunk = [0u8; CHUNK];
        while self.open && total < MAX_READ_PER_CALL {
            let want = read_budget(total);
            match self.stream.read(chunk.get_mut(..want).unwrap_or(&mut [])) {
                Ok(0) => {
                    // End of stream. Not an error — see `is_hangup`.
                    self.open = false;
                    break;
                }
                Ok(n) => {
                    // `n <= chunk.len()` is guaranteed by `Read::read`, but a
                    // broken `Read` impl is exactly the kind of thing a socket
                    // layer should survive rather than trust: `get` turns "the
                    // OS lied about how much it read" into a short read
                    // instead of a panic in the middle of the event loop.
                    let Some(filled) = chunk.get(..n) else { break };
                    buf.extend_from_slice(filled);
                    total = total.saturating_add(n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // A signal cut the call short before it read anything. The
                // loop simply goes round again.
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if is_hangup(e.kind()) => {
                    self.open = false;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut sent = 0usize;
        while sent < bytes.len() {
            // The socket is non-blocking, so a write can be short or refuse
            // outright. `write_all` cannot be used here: it treats `WouldBlock`
            // as an error and would abandon a frame half-sent, which for a
            // length-prefixed protocol desynchronises the stream permanently.
            match self.stream.write(bytes.get(sent..).unwrap_or(&[])) {
                Ok(0) => {
                    self.open = false;
                    return Err(io::Error::new(
                        ErrorKind::WriteZero,
                        "the compositor accepted no bytes",
                    ));
                }
                Ok(n) => sent = sent.saturating_add(n),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // The peer's receive window is full. Finishing the frame is
                    // the only correct answer — a frame cut in half is worse
                    // than no frame — so the rest goes out in blocking mode and
                    // this call takes as long as the peer needs.
                    return self.finish_write(bytes.get(sent..).unwrap_or(&[]));
                }
                Err(e) if is_hangup(e.kind()) => {
                    // The peer is gone. The bytes are lost, but nothing is
                    // wrong: the loop above this one ends on `is_open`, and
                    // reporting a failure here would turn every compositor
                    // shutdown into an application crash report.
                    self.open = false;
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn wait(&mut self) -> io::Result<()> {
        if !self.open {
            // Nothing will ever arrive; parking would hang the caller's loop
            // exactly when it is trying to notice the connection ended.
            return Ok(());
        }
        if let Some(wake) = self.wake.as_mut() {
            // Two things to wait on, which a blocking `peek` cannot do. Only a
            // socket that has handed out a waker takes this path: a lone
            // blocking read is the one wait the platform can always carry out
            // without polling — on SlateOS it parks in the network daemon,
            // where a `poll` of the same socket is rescanned on a backoff.
            wake.set.clear();
            wake.set.add_source(&self.stream);
            let woken = wake.set.add_source(&wake.receiver);
            wake.set.wait(self.wait_timeout)?;
            if wake.set.is_ready(woken) {
                wake.receiver.drain();
            }
            // Whatever made the stream ready — bytes, or the peer hanging up —
            // the caller's next `read` finds, as it does after a `peek`.
            return Ok(());
        }
        self.stream.set_nonblocking(false)?;
        let parked = self.park();
        // Restored on every path, including the failing one: a socket left
        // blocking would make the next `read` block, which the trait forbids
        // and which would freeze the application on an idle desktop.
        let restored = self.stream.set_nonblocking(true);
        parked.and(restored)
    }

    /// Cap how long [`Transport::wait`] parks.
    ///
    /// Only useful to a caller that has something to do on a timer — an
    /// animation, a blinking caret — since input alone already wakes the wait.
    /// [`EventLoop`](../../oswindow/struct.EventLoop.html) is that caller: it
    /// sets this from the nearest registered wake-up before every park.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidInput`] for a zero duration, which the platform
    /// would read as "no timeout" and which therefore means the opposite of
    /// what a caller passing it intends.
    fn set_wait_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if timeout.is_some_and(|d| d.is_zero()) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "a zero wait timeout means 'never time out', which is not what a caller means",
            ));
        }
        self.wait_timeout = timeout;
        Ok(())
    }

    /// A pipe (or, on Windows, a loopback socket pair) that ends
    /// [`Transport::wait`] when written — made on first request and shared by
    /// every waker handed out after it.
    ///
    /// # Errors
    ///
    /// Whatever making the pipe fails with — out of descriptors, most likely.
    fn waker(&mut self) -> io::Result<Option<Waker>> {
        if self.wake.is_none() {
            let (sender, receiver) = wake_channel()?;
            self.wake = Some(Wake {
                receiver,
                sender: Arc::new(sender),
                set: WaitSet::new(),
            });
        }
        Ok(self
            .wake
            .as_ref()
            .map(|wake| Waker::from(Arc::clone(&wake.sender))))
    }
}

/// How long a write will wait on a peer that has stopped reading.
///
/// Bounded, unlike the read wait: having nothing to read is the normal state of
/// an idle desktop, whereas a peer that will not drain is a peer in trouble,
/// and an application blocked on it for ever cannot even report that.
const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);

impl Tcp {
    /// Send the tail of a frame the non-blocking path could not fit.
    ///
    /// Reached only when a frame is larger than the space left in the peer's
    /// receive window — a full-desktop scene frame meeting a slow reader, not a
    /// keystroke.
    fn finish_write(&mut self, rest: &[u8]) -> io::Result<()> {
        self.stream.set_nonblocking(false)?;
        let result = self.finish_write_blocking(rest);
        // Restored on every path: a socket left blocking would make the next
        // `read` block, which the trait forbids.
        let restored = self.stream.set_nonblocking(true);
        result.and(restored)
    }

    /// The body of [`Self::finish_write`], with the socket already blocking.
    /// Split out so the mode is restored even when this fails.
    fn finish_write_blocking(&mut self, rest: &[u8]) -> io::Result<()> {
        self.stream.set_write_timeout(Some(WRITE_STALL_TIMEOUT))?;
        match self.stream.write_all(rest) {
            Ok(()) => Ok(()),
            Err(e) if is_hangup(e.kind()) => {
                self.open = false;
                Ok(())
            }
            // A timeout lands here, and is deliberately an error rather than a
            // silent truncation: the peer has part of a frame and the stream
            // can never be parsed again, so the caller must learn the
            // connection is finished rather than keep writing into it.
            Err(e) => {
                self.open = false;
                Err(e)
            }
        }
    }
}

impl AsWaitHandle for Tcp {
    /// The stream underneath, so a server can wait on many of these at once
    /// ([`WaitSet`]) rather than parking on one with
    /// [`Transport::wait`].
    fn wait_handle(&self) -> WaitHandle {
        self.stream.wait_handle()
    }
}

/// Whether `accept` failing with `kind` is about the one connection it was
/// taking rather than about the listener: a peer that reset or abandoned its
/// connection while it sat in the queue, which some platforms report from
/// `accept` itself, or a signal cutting the call short. Ending the compositor
/// over one would let any program that can connect and hang up quickly take
/// the desktop down.
const fn accept_error_is_the_peers(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset | ErrorKind::Interrupted
    )
}

/// The compositor's end: where clients connect, handed back as [`Socket`]s.
///
/// A TCP listener, a SlateOS service, or both. The default display on SlateOS
/// is both ([`Self::bind_display`]): the service for the machine's own
/// programs, each of which the kernel names, and TCP for everything else.
///
/// Non-blocking, because a compositor has a frame to composite whether or not
/// anyone is connecting. [`Self::accept`] returns `None` rather than parking.
pub struct Listener {
    /// The TCP listener, unless this one listens only as a service.
    tcp: Option<TcpListener>,
    /// The service local clients connect to, and its name.
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    service: Option<(String, crate::channel::ChannelListener)>,
    /// Why the default display is not also served as the display service:
    /// the kernel's refusal, when [`Self::bind_display`] tried.
    service_refusal: Option<io::Error>,
    /// Which carrier [`Self::accept`] asks first. Alternated, so a stream of
    /// connections on one cannot keep a connection on the other waiting.
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    service_first: bool,
}

impl Listener {
    /// Listen on `addr` over TCP.
    ///
    /// # Errors
    ///
    /// Whatever binding fails with — most often that something else already
    /// holds the port, which for the default address means a compositor is
    /// already running.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let tcp = TcpListener::bind(addr)?;
        tcp.set_nonblocking(true)?;
        Ok(Self::with_tcp(Some(tcp)))
    }

    const fn with_tcp(tcp: Option<TcpListener>) -> Self {
        Self {
            tcp,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            service: None,
            service_refusal: None,
            #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
            service_first: true,
        }
    }

    /// Listen wherever [`SLATE_DISPLAY`](DISPLAY_VAR) says this display lives:
    /// the mirror of [`Socket::connect_display`], by the same rules.
    ///
    /// Unset, that is the default display — [`DEFAULT_DISPLAY`] over TCP and,
    /// on SlateOS, the display service
    /// ([`DISPLAY_SERVICE`](crate::channel::DISPLAY_SERVICE)) as well. Failing
    /// to register the service does not fail this: TCP clients are still
    /// served, and a client that finds no service falls back to TCP. The
    /// refusal is kept for the caller to report ([`Self::service_refusal`]).
    ///
    /// # Errors
    ///
    /// As [`Self::bind_to_display`], plus the environment error
    /// [`display_addr`] reports.
    pub fn bind_display() -> io::Result<Self> {
        match std::env::var_os(DISPLAY_VAR) {
            Some(_) => Self::bind_to_display(&display_addr()?),
            None => Self::bind_default(),
        }
    }

    /// Listen at `display`, written as [`SLATE_DISPLAY`](DISPLAY_VAR) is: a
    /// TCP address, or `service:NAME` to listen only as that SlateOS service.
    ///
    /// # Errors
    ///
    /// As [`Self::bind`] for an address and [`Self::serve`] for a service.
    pub fn bind_to_display(display: &str) -> io::Result<Self> {
        match display.strip_prefix(SERVICE_PREFIX) {
            Some(name) => Self::serve(name),
            None => Self::bind(display),
        }
    }

    /// The default display: see [`Self::bind_display`].
    fn bind_default() -> io::Result<Self> {
        let listener = Self::bind(DEFAULT_DISPLAY)?;
        #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
        let listener = {
            let mut listener = listener;
            if let Err(e) = listener.register_service(crate::channel::DISPLAY_SERVICE) {
                listener.service_refusal = Some(e);
            }
            listener
        };
        Ok(listener)
    }

    /// Listen only as the SlateOS service `name`: local clients, each named
    /// by the kernel ([`Socket::peer_cred`]), and nothing from the network.
    ///
    /// # Errors
    ///
    /// As [`Self::register_service`].
    pub fn serve(name: &str) -> io::Result<Self> {
        let mut listener = Self::with_tcp(None);
        listener.register_service(name)?;
        Ok(listener)
    }

    /// Also listen as the SlateOS service `name`.
    ///
    /// # Errors
    ///
    /// `EACCES` without the `Service` capability with `WRITE` on it,
    /// `EADDRINUSE` if something else holds the name — most likely another
    /// compositor — and the kernel's other errnos;
    /// [`ErrorKind::AlreadyExists`] if this listener already serves a
    /// service, and [`ErrorKind::Unsupported`] anywhere but SlateOS.
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    pub fn register_service(&mut self, name: &str) -> io::Result<()> {
        if let Some((held, _)) = &self.service {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("already listening as the service {held:?}"),
            ));
        }
        let service = crate::channel::ChannelListener::register(name)?;
        self.service = Some((name.to_owned(), service));
        self.service_refusal = None;
        Ok(())
    }

    /// Services are SlateOS's; anywhere else there is nothing to register.
    ///
    /// # Errors
    ///
    /// Always [`ErrorKind::Unsupported`].
    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    pub fn register_service(&mut self, name: &str) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            format!("cannot listen as the service {name:?}: only SlateOS has services"),
        ))
    }

    /// The SlateOS service this listens as, if any.
    #[must_use]
    pub fn service_name(&self) -> Option<&str> {
        #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
        {
            self.service.as_ref().map(|(name, _)| name.as_str())
        }
        #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
        {
            None
        }
    }

    /// Why [`Self::bind_display`] is not also serving the display service —
    /// the kernel's answer, for the caller to report. `None` when it is, or
    /// when it never tried.
    #[must_use]
    pub const fn service_refusal(&self) -> Option<&io::Error> {
        self.service_refusal.as_ref()
    }

    /// The TCP address actually bound — the way to learn the port when `bind`
    /// was given `:0`.
    ///
    /// # Errors
    ///
    /// If the socket is not bound, and [`ErrorKind::Unsupported`] for a
    /// listener that listens only as a service, which has no address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.tcp {
            Some(tcp) => tcp.local_addr(),
            None => Err(io::Error::new(
                ErrorKind::Unsupported,
                "listening only as a service, which has no address",
            )),
        }
    }

    /// Add every handle that is readable when a connection is waiting — the
    /// TCP listener's and the service's — to `set`, and say where they went.
    ///
    /// Into a caller's set rather than returned, so that a server rebuilding
    /// its set before every wait allocates nothing to learn them.
    pub fn wait_on(&self, set: &mut WaitSet) -> std::ops::Range<usize> {
        let first = set.len();
        if let Some(tcp) = &self.tcp {
            set.add_source(tcp);
        }
        #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
        if let Some((_, service)) = &self.service {
            set.add_source(service);
        }
        first..set.len()
    }

    /// Take the next pending connection, or `None` if there is none right now.
    ///
    /// # Errors
    ///
    /// Whatever accepting fails with, excluding the "nothing pending" case,
    /// which is `Ok(None)` — an empty accept queue is the ordinary state of a
    /// running desktop, not a failure — and excluding a connection that ended
    /// before it could be accepted, which is the peer's business and is also
    /// `Ok(None)`: the rest of the queue is still there for the next call.
    pub fn accept(&mut self) -> io::Result<Option<Socket>> {
        #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
        {
            let service_first = self.service_first;
            self.service_first = !service_first;
            if service_first {
                if let Some(socket) = self.accept_service()? {
                    return Ok(Some(socket));
                }
                self.accept_tcp()
            } else {
                if let Some(socket) = self.accept_tcp()? {
                    return Ok(Some(socket));
                }
                self.accept_service()
            }
        }
        #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
        {
            self.accept_tcp()
        }
    }

    fn accept_tcp(&self) -> io::Result<Option<Socket>> {
        let Some(tcp) = &self.tcp else {
            return Ok(None);
        };
        match tcp.accept() {
            Ok((stream, _addr)) => Ok(Some(Socket::adopt(stream)?)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) if accept_error_is_the_peers(e.kind()) => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    fn accept_service(&self) -> io::Result<Option<Socket>> {
        let Some((_, service)) = &self.service else {
            return Ok(None);
        };
        Ok(service.accept()?.map(|conn| Socket {
            carrier: Carrier::Channel(conn),
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use std::thread;
    use std::time::Instant;

    use guitk::event::Event;

    use super::*;
    use crate::client::Connection;
    use crate::control::{
        RequestBody, ResponseBody, WindowSpec, decode_requests, encode_responses,
    };
    use crate::input::{InputEvent, encode_input_frame};

    /// A listener on a kernel-chosen port and a client connected to it.
    ///
    /// Port zero rather than a fixed number: these tests run concurrently with
    /// each other and with whatever else is on the machine, and a hard-coded
    /// port makes a test suite that fails depending on what else is running.
    fn connected_pair() -> (Socket, Socket) {
        let mut listener = Listener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("bound address");
        let client = Socket::connect(addr).expect("connect");
        // The listener is non-blocking, so accept until the pending connection
        // appears. It is already in flight — this is a handful of iterations at
        // most, and the loop is bounded so a genuine failure is a failure and
        // not a hang.
        for _ in 0..1000 {
            if let Some(server) = listener.accept().expect("accept") {
                return (client, server);
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the connection never arrived at the listener");
    }

    /// Read until `want` bytes have accumulated, or give up. The socket is a
    /// stream: one write is not one read, so a test that read once would pass
    /// or fail on timing.
    fn read_at_least(sock: &mut Socket, want: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        for _ in 0..1000 {
            sock.read(&mut buf).expect("read");
            if buf.len() >= want {
                return buf;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("only {} of {want} bytes arrived", buf.len());
    }

    #[test]
    fn what_one_end_writes_the_other_reads() {
        let (mut a, mut b) = connected_pair();
        a.write(b"hello").unwrap();
        let buf = read_at_least(&mut b, 5);
        assert_eq!(&buf[..5], b"hello");
    }

    #[test]
    fn an_idle_socket_reads_zero_rather_than_blocking() {
        // The trait's central requirement. A `read` that blocked here would
        // deadlock every event loop in the tree on an idle desktop.
        let (mut a, _b) = connected_pair();
        let mut buf = Vec::new();
        assert_eq!(a.read(&mut buf).unwrap(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn the_two_directions_do_not_share_a_queue() {
        let (mut a, mut b) = connected_pair();
        a.write(b"from a").unwrap();
        b.write(b"from b").unwrap();
        let at_a = read_at_least(&mut a, 6);
        let at_b = read_at_least(&mut b, 6);
        assert_eq!(&at_a[..6], b"from b");
        assert_eq!(&at_b[..6], b"from a");
    }

    #[test]
    fn a_hang_up_is_seen_as_a_close_and_not_as_an_error() {
        let (mut a, mut b) = connected_pair();
        assert!(a.is_open());
        b.close();
        // The end of the stream reaches the peer as a zero-length read, which
        // must not be reported as a failure — a compositor exiting is not a
        // crash in the application.
        for _ in 0..1000 {
            let mut buf = Vec::new();
            a.read(&mut buf).expect("a hang-up is not a read error");
            if !a.is_open() {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the hang-up was never noticed");
    }

    #[test]
    fn wait_returns_once_a_byte_arrives() {
        // The whole reason `wait` exists: it must park until there is something
        // and then come back. A `wait` that returned immediately would spin;
        // one that never returned would hang.
        let (mut a, mut b) = connected_pair();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            b.write(b"x").unwrap();
            // Held open, so what wakes the wait is the byte and not the
            // hang-up — otherwise this would pass with a broken `peek`.
            thread::sleep(Duration::from_millis(300));
            b
        });
        a.wait().expect("wait");
        let mut buf = Vec::new();
        a.read(&mut buf).expect("read after wait");
        assert_eq!(buf, b"x");
        drop(writer.join().expect("writer thread"));
    }

    #[test]
    fn a_bounded_wait_parks_for_the_bound_and_then_comes_back() {
        // The one thing about the frame clock that a synthetic clock cannot
        // check: that the park really is bounded, and really is a park. The
        // peer is alive and silent, so an unbounded `wait` would block until
        // the test harness killed it, and a `wait` that ignored the bound by
        // returning at once would report an implausibly short interval.
        //
        // `oswindow::EventLoop` is the caller this exists for: it sets the
        // bound from the nearest registered wake-up before every park, so an
        // animation frame arrives on time without the loop polling for it.
        let (mut a, _b) = connected_pair();
        a.set_wait_timeout(Some(Duration::from_millis(60))).unwrap();
        let started = Instant::now();
        a.wait().expect("wait");
        let parked = started.elapsed();
        assert!(
            parked >= Duration::from_millis(40),
            "came back after {parked:?}, which is too soon to have parked at all"
        );
        assert!(
            parked < Duration::from_secs(5),
            "still parked after {parked:?}; the bound was ignored"
        );
    }

    #[test]
    fn wait_leaves_the_socket_non_blocking() {
        // The bug this catches is a `wait` that restores the mode only on its
        // success path: the next `read` would then block for ever, and it would
        // do so only after a timeout or an error, which is the hardest kind of
        // failure to reproduce.
        let (mut a, _b) = connected_pair();
        a.set_wait_timeout(Some(Duration::from_millis(10))).unwrap();
        a.wait().expect("wait");
        let mut buf = Vec::new();
        assert_eq!(a.read(&mut buf).unwrap(), 0, "read blocked or errored");
    }

    #[test]
    fn wait_on_a_closed_socket_returns_at_once() {
        let (mut a, _b) = connected_pair();
        a.close();
        a.wait().expect("wait on a closed socket");
    }

    #[test]
    fn a_zero_wait_timeout_is_refused() {
        let (mut a, _b) = connected_pair();
        assert!(a.set_wait_timeout(Some(Duration::ZERO)).is_err());
        assert!(a.set_wait_timeout(Some(Duration::from_millis(1))).is_ok());
        assert!(a.set_wait_timeout(None).is_ok());
    }

    #[test]
    fn a_real_request_and_reply_cross_the_socket_intact() {
        // The point of the whole module: a genuine round trip between two
        // sockets, both sides running the real codecs.
        let (client_end, mut server_end) = connected_pair();
        let mut conn = Connection::new(client_end);

        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Notes", 640, 480,
            )))
            .unwrap();

        let wire = read_at_least(&mut server_end, 1);
        let (reqs, used) = decode_requests(&wire).unwrap();
        assert_eq!(used, wire.len());
        assert_eq!(reqs[0].seq, seq);

        server_end
            .write(&encode_responses(&[crate::control::Response::new(
                seq,
                ResponseBody::WindowCreated { window: 5 },
            )]))
            .unwrap();

        // `round_trip` would do this, but pumping explicitly keeps the test
        // from hanging if the reply never comes.
        for _ in 0..1000 {
            conn.pump().unwrap();
            if let Some(reply) = conn.take_reply(seq) {
                assert_eq!(reply, ResponseBody::WindowCreated { window: 5 });
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the reply never arrived");
    }

    #[test]
    fn a_frame_written_in_pieces_still_arrives_whole() {
        // A socket is a byte stream and does not respect frame boundaries. The
        // reassembly lives in `Connection`; this proves the transport hands it
        // the pieces rather than losing or reordering them.
        let (client_end, mut server_end) = connected_pair();
        let mut conn = Connection::new(client_end);
        let frame = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        let mid = frame.len() / 2;

        server_end.write(&frame[..mid]).unwrap();
        // Half a frame is not a frame, however many times it is pumped.
        for _ in 0..20 {
            assert_eq!(conn.pump().unwrap(), 0);
            thread::sleep(Duration::from_millis(1));
        }
        server_end.write(&frame[mid..]).unwrap();
        for _ in 0..1000 {
            if conn.pump().unwrap() == 1 {
                assert_eq!(conn.pending_events(), 1);
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the completed frame never decoded");
    }

    #[test]
    fn a_large_write_survives_a_slow_reader() {
        // Bigger than any socket buffer, so the write path's `WouldBlock`
        // branch is actually taken. If that branch dropped the remainder — the
        // obvious wrong answer — the reader would see a truncated stream, which
        // for a length-prefixed protocol is unrecoverable rather than merely
        // lossy.
        let payload: Vec<u8> = (0..2_000_000usize)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let (mut a, mut b) = connected_pair();
        let expected = payload.clone();
        let reader = thread::spawn(move || {
            let mut got = Vec::new();
            for _ in 0..20_000 {
                b.read(&mut got).expect("read");
                if got.len() >= expected.len() {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            got
        });
        a.write(&payload).unwrap();
        let got = reader.join().expect("reader thread");
        assert_eq!(got.len(), payload.len(), "byte count differs");
        assert_eq!(got, payload, "bytes differ");
    }

    #[test]
    fn one_read_is_bounded_so_a_fast_peer_cannot_starve_dispatch() {
        // A `read` that drained the socket completely could be held there by a
        // peer writing faster than this process dispatches, and the events
        // already read would never be acted on. Every individual call must
        // return, and nothing may be lost by its returning.
        let total = MAX_READ_PER_CALL * 3;
        let payload: Vec<u8> = (0..total).map(|i| u8::try_from(i % 251).unwrap()).collect();
        let (mut a, mut b) = connected_pair();
        let sending = payload.clone();
        let writer = thread::spawn(move || {
            b.write(&sending).unwrap();
            b
        });

        let mut got = Vec::new();
        for _ in 0..20_000 {
            let before = got.len();
            let n = a.read(&mut got).expect("read");
            assert!(n <= MAX_READ_PER_CALL, "one read returned {n} bytes");
            assert_eq!(got.len(), before + n, "the count and the bytes disagree");
            if got.len() >= total {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        drop(writer.join().expect("writer thread"));
        assert_eq!(got.len(), total, "byte count differs");
        assert_eq!(got, payload, "bytes differ");
    }

    /// The cap holds from *every* starting total, not just the ones a run of
    /// full-length reads can reach.
    ///
    /// `one_read_is_bounded_so_a_fast_peer_cannot_starve_dispatch` above is the
    /// test that found the bug, and it found it twice in a loaded workspace run
    /// and never once in 128 attempts aimed at it — because reaching an
    /// off-grid `total` needs the OS to deschedule the writer at the right
    /// moment. This one reaches every off-grid total on purpose.
    #[test]
    fn the_read_budget_never_lets_a_chunk_cross_the_cap() {
        for total in 0..=MAX_READ_PER_CALL {
            let want = read_budget(total);
            assert!(want <= CHUNK, "budget at {total} exceeds one chunk");
            assert!(
                total.saturating_add(want) <= MAX_READ_PER_CALL,
                "budget at {total} overshoots the cap"
            );
        }

        // On the grid, the budget is a whole chunk and the walk ends exactly on
        // the cap — the case that used to make the unclamped loop look correct.
        assert_eq!(read_budget(0), CHUNK);
        assert_eq!(read_budget(MAX_READ_PER_CALL - CHUNK), CHUNK);

        // Off the grid. 5,024 bytes of remaining budget is the state lane B's
        // failure trace showed; the unclamped loop read a whole 8,192 there and
        // returned 265,312 against a 262,144 cap.
        assert_eq!(read_budget(MAX_READ_PER_CALL - 5_024), 5_024);

        // Past the cap the loop's own guard has already stopped it, but the
        // budget must still be a refusal rather than a wrap.
        assert_eq!(read_budget(MAX_READ_PER_CALL), 0);
        assert_eq!(read_budget(usize::MAX), 0);
    }

    #[test]
    fn the_default_display_is_loopback() {
        // A display server reachable from the network by default would ship
        // every keystroke on this machine to anyone who asked.
        assert!(DEFAULT_DISPLAY.starts_with("127.0.0.1:"));
    }

    #[test]
    fn an_unset_display_variable_gives_the_default() {
        // Read rather than set: the environment is process-wide and these tests
        // share it, so a test that set the variable would corrupt every other
        // test running at that moment.
        if std::env::var_os(DISPLAY_VAR).is_none() {
            assert_eq!(display_addr().unwrap(), DEFAULT_DISPLAY);
        }
    }

    #[test]
    fn a_listener_reports_the_port_it_actually_got() {
        let mut listener = Listener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        assert_ne!(addr.port(), 0, "port zero means the kernel chose one");
        assert!(
            listener.accept().expect("accept").is_none(),
            "nobody dialled"
        );
    }

    // ---- Services: SlateOS's, and a mistake to report anywhere else ----

    #[test]
    fn a_service_display_is_refused_off_slateos_rather_than_dialled() {
        // `service:...` parses as no TCP address at all; reaching the resolver
        // with it would report a confusing lookup failure instead.
        let dialled = Socket::connect_to_display("service:org.slateos.Display");
        assert_eq!(
            dialled.err().map(|e| e.kind()),
            Some(ErrorKind::Unsupported)
        );
        let listened = Listener::bind_to_display("service:org.slateos.Display");
        assert_eq!(
            listened.err().map(|e| e.kind()),
            Some(ErrorKind::Unsupported)
        );
        assert_eq!(
            Listener::serve("org.slateos.Display")
                .err()
                .map(|e| e.kind()),
            Some(ErrorKind::Unsupported)
        );
    }

    #[test]
    fn a_tcp_listener_can_not_also_become_a_service_off_slateos() {
        let mut listener = Listener::bind("127.0.0.1:0").expect("bind");
        let refused = listener.register_service("org.slateos.Display");
        assert_eq!(refused.map_err(|e| e.kind()), Err(ErrorKind::Unsupported));
        assert_eq!(listener.service_name(), None);
        assert!(
            listener.service_refusal().is_none(),
            "only bind_display records a refusal; a caller that asked has its own answer"
        );
        // And it still listens: the refusal cost it nothing.
        assert!(listener.local_addr().is_ok());
    }

    #[test]
    fn a_tcp_address_display_listens_over_tcp() {
        let mut listener = Listener::bind_to_display("127.0.0.1:0").expect("bind");
        let addr = listener
            .local_addr()
            .expect("a TCP listener has an address");
        let _client = Socket::connect(addr).expect("connect");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if listener.accept().expect("accept").is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the connection never arrived");
    }

    #[test]
    fn a_listener_waits_on_exactly_its_own_handles_and_says_where() {
        let mut listener = Listener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap();
        let mut set = WaitSet::new();
        let (_a, _b) = connected_pair();
        // Something already in the set, so the range is not trivially `0..`.
        let (other, _peer) = connected_pair();
        set.add_source(&other);
        let range = listener.wait_on(&mut set);
        assert_eq!(
            range,
            1..2,
            "one TCP listener, after the one handle before it"
        );
        assert_eq!(set.len(), 2);

        // Nobody dialling: the listener's slot is not ready.
        set.wait(Some(Duration::from_millis(20))).unwrap();
        assert!(!set.is_ready(range.start));

        let _client = Socket::connect(addr).expect("connect");
        set.wait(Some(Duration::from_secs(5))).unwrap();
        assert!(
            set.is_ready(range.start),
            "a pending connection makes the listener's handle ready"
        );
        assert!(listener.accept().unwrap().is_some());
    }

    #[test]
    fn a_tcp_socket_names_an_address_and_no_process() {
        let (client, server) = connected_pair();
        assert_eq!(client.peer_addr().unwrap(), server.local_addr().unwrap());
        assert_eq!(
            client.peer_cred(),
            None,
            "the kernel cannot vouch for a TCP peer, so nothing is claimed"
        );
        assert_eq!(server.peer_cred(), None);
        assert_eq!(server.peer_has_key(), None);
    }

    #[test]
    fn a_hung_up_tcp_accept_is_not_a_failure_of_the_listener() {
        // The kinds `accept` may report for a connection its peer abandoned in
        // the queue. Each must read as "nothing to accept", or one program
        // connecting and resetting would end the compositor.
        for kind in [
            ErrorKind::ConnectionAborted,
            ErrorKind::ConnectionReset,
            ErrorKind::Interrupted,
        ] {
            assert!(accept_error_is_the_peers(kind), "{kind:?}");
        }
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::OutOfMemory,
            ErrorKind::Other,
        ] {
            assert!(!accept_error_is_the_peers(kind), "{kind:?}");
        }
    }

    #[test]
    fn a_waker_ends_a_wait_from_another_thread() {
        let (_client, mut server) = connected_pair();
        let waker = server.waker().unwrap().expect("a socket can be woken");
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            waker.wake();
        });
        // Nothing will arrive on the wire: only the wake can end this before
        // the minute is up.
        server
            .set_wait_timeout(Some(Duration::from_mins(1)))
            .unwrap();
        let began = Instant::now();
        server.wait().unwrap();
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the wake did not end the wait"
        );
        worker.join().unwrap();
        assert!(server.is_open(), "a wake is not a hang-up");
    }

    #[test]
    fn a_consumed_wake_does_not_wake_the_next_wait() {
        let (_client, mut server) = connected_pair();
        let waker = server.waker().unwrap().unwrap();
        waker.wake_by_ref();
        server.wait().unwrap();
        server
            .set_wait_timeout(Some(Duration::from_millis(40)))
            .unwrap();
        let began = Instant::now();
        server.wait().unwrap();
        assert!(
            began.elapsed() >= Duration::from_millis(40),
            "one wake ended two waits"
        );
    }

    #[test]
    fn a_socket_with_a_waker_still_wakes_for_bytes() {
        let (mut client, mut server) = connected_pair();
        let _waker = server.waker().unwrap().unwrap();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            client.write(b"hi").unwrap();
            client
        });
        server.wait().unwrap();
        let _client = writer.join().unwrap();
        let buf = read_at_least(&mut server, 2);
        assert_eq!(&buf[..2], b"hi");
    }

    #[test]
    fn every_waker_a_socket_hands_out_wakes_the_same_wait() {
        let (_client, mut server) = connected_pair();
        let first = server.waker().unwrap().unwrap();
        let second = server.waker().unwrap().unwrap();
        assert!(first.will_wake(&second), "two pipes for one socket");
        second.wake();
        let began = Instant::now();
        server
            .set_wait_timeout(Some(Duration::from_mins(1)))
            .unwrap();
        server.wait().unwrap();
        assert!(began.elapsed() < Duration::from_secs(30));
        drop(first);
    }

    #[test]
    fn a_waker_outliving_its_socket_wakes_nothing_and_does_not_fail() {
        // The worker finishes after the window has closed: its wake goes
        // nowhere, and must neither panic nor block.
        let (_client, mut server) = connected_pair();
        let waker = server.waker().unwrap().unwrap();
        drop(server);
        waker.wake();
    }
}
