//! The first transport that crosses a process boundary.
//!
//! [`loopback`](crate::loopback) proved the codecs against each other inside
//! one process; it cannot carry a frame between two. This module can. A
//! [`Socket`] is a connected TCP stream that implements [`Transport`], and a
//! [`Listener`] is the compositor's end: it accepts them.
//!
//! ## Why TCP, and why that is not a stopgap
//!
//! This crate is, by its own first line, a *remote*-desktop protocol. A
//! transport that only worked between two processes on one machine would
//! contradict the thing the protocol exists for. TCP is also the one carrier
//! that behaves identically on the hosted development build and on SlateOS
//! itself once its network stack is running, so the same client code is
//! exercised in both places rather than one path being tested and the other
//! merely written.
//!
//! A local connection pays for that with a loopback round trip instead of a
//! kernel channel. On loopback that is a memory copy through the network stack,
//! measured in single-digit microseconds — the same order as the IPC it stands
//! in for, and far below a frame budget. When SlateOS's own channel IPC becomes
//! reachable from a userspace application, it becomes a second implementation
//! of [`Transport`] beside this one; nothing above the trait changes, which is
//! the reason the trait is where it is.
//!
//! ## Where the compositor is
//!
//! [`display_addr`] answers that, from the `SLATE_DISPLAY` environment variable
//! or [`DEFAULT_DISPLAY`] when it is unset — the same arrangement as X11's
//! `DISPLAY`, for the same reason: an application must not have the address of
//! its display server compiled into it.
//!
//! ## Blocking discipline
//!
//! [`Transport::read`] must not block and [`Transport::wait`] must. A socket
//! cannot be both at once, so a [`Socket`] is held in non-blocking mode and
//! `wait` briefly switches it back, [`peek`](TcpStream::peek)s one byte — which
//! blocks until a byte is *available* without consuming it — and switches
//! forward again. The alternative, polling on a timer, would either add latency
//! to every keystroke or wake an idle desktop hundreds of times a second.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::client::Transport;

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

/// The compositor's address, from the environment or the default.
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
/// — rather than for the family it currently uses. When a SlateOS channel
/// transport joins it, applications that say `socket::connect_display()` will
/// not have named TCP anywhere.
pub struct Socket {
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
}

impl Socket {
    /// Dial `addr`.
    ///
    /// # Errors
    ///
    /// Whatever the connection attempt fails with; also
    /// [`ErrorKind::InvalidInput`] if `addr` resolves to nothing.
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        Self::adopt(TcpStream::connect(addr)?)
    }

    /// Dial whatever [`display_addr`] names.
    ///
    /// # Errors
    ///
    /// As [`Self::connect`], plus the environment error [`display_addr`]
    /// reports.
    pub fn connect_display() -> io::Result<Self> {
        Self::connect(display_addr()?)
    }

    /// Take over an already-connected stream — the listener's side.
    ///
    /// # Errors
    ///
    /// If the stream cannot be put into the mode this transport requires.
    pub fn adopt(stream: TcpStream) -> io::Result<Self> {
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
        })
    }

    /// The peer's address.
    ///
    /// # Errors
    ///
    /// If the socket has no peer — it has already been shut down.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    /// The local address.
    ///
    /// # Errors
    ///
    /// If the socket is not bound.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }

    /// Hang up, so both this side and the peer see the connection end.
    pub fn close(&mut self) {
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

impl Transport for Socket {
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
}

/// How long a write will wait on a peer that has stopped reading.
///
/// Bounded, unlike the read wait: having nothing to read is the normal state of
/// an idle desktop, whereas a peer that will not drain is a peer in trouble,
/// and an application blocked on it for ever cannot even report that.
const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);

impl Socket {
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

/// The compositor's end: a listening socket that hands back [`Socket`]s.
///
/// Non-blocking, because a compositor has a frame to composite whether or not
/// anyone is connecting. [`Self::accept`] returns `None` rather than parking.
pub struct Listener {
    inner: TcpListener,
}

impl Listener {
    /// Listen on `addr`.
    ///
    /// # Errors
    ///
    /// Whatever binding fails with — most often that something else already
    /// holds the port, which for the default address means a compositor is
    /// already running.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let inner = TcpListener::bind(addr)?;
        inner.set_nonblocking(true)?;
        Ok(Self { inner })
    }

    /// Listen wherever [`display_addr`] says this display lives.
    ///
    /// # Errors
    ///
    /// As [`Self::bind`], plus the environment error [`display_addr`] reports.
    pub fn bind_display() -> io::Result<Self> {
        Self::bind(display_addr()?)
    }

    /// The address actually bound — the way to learn the port when `bind` was
    /// given `:0`.
    ///
    /// # Errors
    ///
    /// If the socket is not bound.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Take the next pending connection, or `None` if there is none right now.
    ///
    /// # Errors
    ///
    /// Whatever accepting fails with, excluding the "nothing pending" case,
    /// which is `Ok(None)` — an empty accept queue is the ordinary state of a
    /// running desktop, not a failure.
    pub fn accept(&self) -> io::Result<Option<Socket>> {
        match self.inner.accept() {
            Ok((stream, _addr)) => Ok(Some(Socket::adopt(stream)?)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
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
        let listener = Listener::bind("127.0.0.1:0").expect("bind loopback");
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
        let listener = Listener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        assert_ne!(addr.port(), 0, "port zero means the kernel chose one");
        assert!(
            listener.accept().expect("accept").is_none(),
            "nobody dialled"
        );
    }
}
