//! The front end: the loop that actually serves clients.
//!
//! [`wire`](crate::wire) turned a client's bytes into compositor requests and
//! the answers back into bytes, and stopped there — deliberately, because a
//! [`ClientLink`] that owned a socket could not be tested without one. This
//! module is the part that was missing on the other side of that line: it
//! listens, accepts, moves bytes between each socket and its link, and paces
//! the whole thing against the display's refresh rate.
//!
//! With this, the sentence in `known-issues.md` that said both halves of the
//! protocol were complete and nothing carried it between them stops being true.
//! A client process can connect, be given a window, draw into it and have those
//! pixels composited.
//!
//! ## The shape of a tick
//!
//! ```text
//!   accept  →  read → link.receive → Compositor::serve
//!                                          │
//!                            route_input ──┤
//!                                          ▼
//!            write ← link.take_outgoing ← replies + input
//!                                          │
//!                                    compose_frame
//! ```
//!
//! Every step is bounded. A tick accepts at most [`MAX_ACCEPTS_PER_TICK`]
//! connections, each `read` is bounded by the transport, and a link whose
//! undecodable backlog passes [`MAX_PENDING_INPUT`] is dropped. A front end
//! facing untrusted peers that had an unbounded step anywhere would be a peer's
//! choice of how long a frame takes.
//!
//! ## Why it polls
//!
//! A tick runs once per frame interval and returns whether or not anything
//! happened. The obvious alternative — block until a socket is readable —
//! needs a readiness primitive over many descriptors (`poll`, `epoll`, `IOCP`),
//! and the standard library exposes none. The cost is bounded and small: a
//! client's request waits at most one frame interval to be seen, which is the
//! same delay its result would wait for anyway before being composited. It is
//! still waste on a wholly idle desktop, and it is logged as such in
//! `known-issues.md` → `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use guiremote::socket::{Listener, Socket};

use crate::present::{Headless, Present};
use crate::wire::ClientLink;
use crate::{Compositor, Display, WindowId};

/// How many connections one tick will accept.
///
/// A caller that accepted until the queue was empty would let a peer opening
/// connections in a loop hold the compositor there indefinitely, and the
/// desktop would stop drawing. Whatever is left waits one frame.
pub const MAX_ACCEPTS_PER_TICK: usize = 16;

/// How many undecodable bytes a client may have outstanding before it is
/// dropped.
///
/// A frame is length-prefixed, so a client can announce a large one and then
/// send it a byte at a time; the buffer holding the incomplete frame is an
/// allocation whose size the peer chooses. The limit is far above any real
/// frame — a full-desktop scene is on the order of tens of kilobytes — and far
/// below anything that matters to this process.
pub const MAX_PENDING_INPUT: usize = 8 * 1024 * 1024;

/// Why a client's connection ended.
///
/// Kept distinct because they mean different things to whoever reads the log: a
/// hang-up is Tuesday, a protocol error is a bug in some client, and a
/// backlog overrun is either a bug or an attack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disconnect {
    /// The peer closed the connection, or the socket did.
    HungUp,
    /// The socket failed.
    Transport(String),
    /// The client's stream could not be decoded, or it sent a frame only a
    /// compositor sends. Terminal: a stream that has lost frame sync cannot be
    /// resynchronised, because the length that would say how far to skip is
    /// itself part of what is not trusted.
    Protocol(String),
    /// The client's incomplete frame passed [`MAX_PENDING_INPUT`].
    Backlog(usize),
}

impl std::fmt::Display for Disconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HungUp => write!(f, "hung up"),
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
            Self::Backlog(n) => write!(f, "{n} bytes of unfinished frame"),
        }
    }
}

/// Running totals, for a log line and for tests that need to see a decision was
/// taken rather than infer it from a side effect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerStats {
    /// Connections accepted since the server started.
    pub accepted: u64,
    /// Connections that ended, for any of the reasons in [`Disconnect`].
    pub disconnected: u64,
    /// Of those, the ones that ended because the client's stream was bad.
    pub protocol_errors: u64,
    /// Requests and submissions served.
    pub served: u64,
    /// Input events written to some client.
    pub routed_events: u64,
    /// Input events addressed to a window no live connection owned. A non-zero
    /// value is a symptom, not housekeeping — see
    /// [`Compositor::discard_unrouted_input`].
    pub unrouted_events: u64,
    /// Window-list frames pushed to a subscribed shell.
    ///
    /// Should track how often the desktop's window set actually changed, not
    /// the tick rate. A value climbing with `frames` means something is
    /// perturbing the list every tick, which is a bug worth being able to see.
    pub window_lists_sent: u64,
    /// Frames composited.
    pub frames: u64,
    /// Windows destroyed because the client that owned them went away.
    pub orphans_reclaimed: u64,
}

/// One connected client: a socket, and the protocol state for what arrives on
/// it.
struct Client {
    socket: Socket,
    link: ClientLink,
    /// Set during a tick, acted on at the end of it. A client cannot be removed
    /// where the failure is noticed, because its windows must be reclaimed
    /// first and that needs the compositor, which is borrowed by the loop.
    ending: Option<Disconnect>,
}

/// The compositor's listening front end.
pub struct Server {
    listener: Listener,
    clients: Vec<Client>,
    /// Stands in for a process id. A TCP peer cannot be asked what process it
    /// is — there is no `SO_PEERCRED` across a network, and a remote client has
    /// no pid in this machine's namespace at all — so the compositor is given a
    /// per-connection number instead. It is what `ClientLink::client_pid`
    /// carries, and it is unique per connection, which is what the taskbar
    /// actually needs; the day a transport can attest a real pid, this is where
    /// it comes from instead.
    next_client_id: u64,
    stats: ServerStats,
    /// Reused across reads so a busy client does not allocate per tick.
    scratch: Vec<u8>,
}

impl Server {
    /// Listen on `addr`.
    ///
    /// # Errors
    ///
    /// Whatever binding fails with — most often that the port is taken, which
    /// on the default address means a compositor is already running.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        Ok(Self::over(Listener::bind(addr)?))
    }

    /// Listen wherever `SLATE_DISPLAY` says this display lives.
    ///
    /// # Errors
    ///
    /// As [`Self::bind`], plus a malformed `SLATE_DISPLAY`.
    pub fn bind_display() -> io::Result<Self> {
        Ok(Self::over(Listener::bind_display()?))
    }

    /// Serve on an already-bound listener.
    #[must_use]
    pub fn over(listener: Listener) -> Self {
        Self {
            listener,
            clients: Vec::new(),
            // Zero is left free as "no client", matching the convention the
            // rest of the compositor uses for ids that may be absent.
            next_client_id: 1,
            stats: ServerStats::default(),
            scratch: Vec::new(),
        }
    }

    /// The address actually bound — the way to learn the port when `bind` was
    /// given `:0`.
    ///
    /// # Errors
    ///
    /// If the socket is not bound.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// How many clients are connected.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Running totals since the server started.
    #[must_use]
    pub const fn stats(&self) -> &ServerStats {
        &self.stats
    }

    /// Run one round: accept, serve, route, flush, reap.
    ///
    /// Does not compose — [`Self::run`] does that, and a caller driving the
    /// server itself may want to compose on its own schedule.
    ///
    /// # Errors
    ///
    /// Only if *accepting* fails, which is a fault in the listening socket
    /// rather than in any one client. A client's failure ends that client and
    /// is reported through [`ServerStats`], because one misbehaving
    /// application must not be able to stop the desktop.
    pub fn tick(&mut self, compositor: &mut Compositor) -> io::Result<()> {
        self.accept_pending()?;
        self.read_and_serve(compositor);
        self.route_and_flush(compositor);
        self.reap(compositor);
        Ok(())
    }

    /// Take whatever connections are pending, up to the per-tick bound.
    fn accept_pending(&mut self) -> io::Result<()> {
        for _ in 0..MAX_ACCEPTS_PER_TICK {
            let Some(socket) = self.listener.accept()? else {
                break;
            };
            let id = self.next_client_id;
            // Wrapping rather than saturating, and skipping zero: the id is
            // only ever compared for equality, and four billion billion
            // connections is not a situation, but a saturating counter would
            // silently give every later connection the same id.
            self.next_client_id = match self.next_client_id.wrapping_add(1) {
                0 => 1,
                n => n,
            };
            self.clients.push(Client {
                socket,
                link: ClientLink::new(id),
                ending: None,
            });
            self.stats.accepted = self.stats.accepted.saturating_add(1);
        }
        Ok(())
    }

    /// Read from every client and act on whatever completed.
    fn read_and_serve(&mut self, compositor: &mut Compositor) {
        for client in &mut self.clients {
            if client.ending.is_some() {
                continue;
            }
            self.scratch.clear();
            match client.socket.read(&mut self.scratch) {
                Ok(_) => {}
                Err(e) => {
                    client.ending = Some(Disconnect::Transport(e.to_string()));
                    continue;
                }
            }
            client.link.receive(&self.scratch);

            match compositor.serve(&mut client.link) {
                Ok(served) => {
                    self.stats.served = self
                        .stats
                        .served
                        .saturating_add(u64::try_from(served).unwrap_or(u64::MAX));
                }
                Err(e) => {
                    client.ending = Some(Disconnect::Protocol(e.to_string()));
                    continue;
                }
            }

            // Checked after serving, not before: what is left is by definition
            // a frame the decoder could not complete, so this is the peer's
            // unfinished business and not merely a busy tick.
            let pending = client.link.pending_input();
            if pending > MAX_PENDING_INPUT {
                client.ending = Some(Disconnect::Backlog(pending));
                continue;
            }

            // Last, so that anything the client sent before hanging up has
            // already been served. A close noticed first would discard a final
            // `DestroyWindow` and leave the window to be reclaimed by the
            // orphan path instead — the same outcome, reached less tidily.
            if !client.socket.is_open() {
                client.ending = Some(Disconnect::HungUp);
            }
        }
    }

    /// Give every client its input and any window-list change, then write
    /// everything queued for it.
    fn route_and_flush(&mut self, compositor: &mut Compositor) {
        for client in &mut self.clients {
            let routed = compositor.route_input(&mut client.link);
            self.stats.routed_events = self
                .stats
                .routed_events
                .saturating_add(u64::try_from(routed).unwrap_or(u64::MAX));
            // After input, so that a window list and the focus events that
            // caused it reach a shell in the order they happened.
            if compositor.route_window_list(&mut client.link) {
                self.stats.window_lists_sent = self.stats.window_lists_sent.saturating_add(1);
            }
        }
        // Whatever no live link claimed. Counted rather than left to accumulate:
        // an unbounded queue of events for windows nobody owns would eventually
        // be delivered to whoever next opened a window with a recycled id.
        let unrouted = compositor.discard_unrouted_input();
        self.stats.unrouted_events = self
            .stats
            .unrouted_events
            .saturating_add(u64::try_from(unrouted).unwrap_or(u64::MAX));

        for client in &mut self.clients {
            if !client.link.has_outgoing() {
                continue;
            }
            let bytes = client.link.take_outgoing();
            if let Err(e) = client.socket.write(&bytes) {
                // Not overwritten if the client is already ending: the first
                // reason is the one that explains the rest.
                if client.ending.is_none() {
                    client.ending = Some(Disconnect::Transport(e.to_string()));
                }
            }
        }
    }

    /// Remove the clients that ended, destroying the windows they left behind.
    fn reap(&mut self, compositor: &mut Compositor) {
        if !self.clients.iter().any(|c| c.ending.is_some()) {
            return;
        }
        let mut ended = Vec::new();
        self.clients
            .retain_mut(|client| match client.ending.take() {
                None => true,
                Some(reason) => {
                    ended.push((
                        client.link.client_pid(),
                        reason,
                        client.link.windows().to_vec(),
                    ));
                    client.link.close();
                    client.socket.close();
                    false
                }
            });

        for (id, reason, windows) in ended {
            self.stats.disconnected = self.stats.disconnected.saturating_add(1);
            if matches!(reason, Disconnect::Protocol(_) | Disconnect::Backlog(_)) {
                self.stats.protocol_errors = self.stats.protocol_errors.saturating_add(1);
            }
            Self::reclaim(compositor, &windows, &mut self.stats);
            eprintln!("compositor: client {id} disconnected ({reason})");
        }
    }

    /// Destroy the windows a departed client owned.
    ///
    /// Nothing else will: a window outlives the link that opened it, so a
    /// client that crashes with three windows open leaves three windows on
    /// screen that no process can close, move, or draw into.
    fn reclaim(compositor: &mut Compositor, windows: &[WindowId], stats: &mut ServerStats) {
        for &window in windows {
            // The error is "no such window", which here means the client
            // destroyed it before going away. That is the ordinary case and not
            // worth a line in the log.
            if compositor.destroy_window(window).is_ok() {
                stats.orphans_reclaimed = stats.orphans_reclaimed.saturating_add(1);
            }
        }
    }

    /// Composite one frame. Reports whether anything was drawn.
    ///
    /// Separate from [`Self::tick`] because a caller driving the loop itself may
    /// want to compose on its own schedule — but it belongs to the *server*
    /// rather than being left to the caller, so that [`ServerStats::frames`]
    /// counts the same thing no matter who drives. It previously lived inline in
    /// [`Self::run`], which meant a test that ticked and composed by hand saw a
    /// frame count of zero while the screen was demonstrably being drawn: a
    /// statistic that is only true for one of its two callers is a statistic
    /// that will eventually be believed by the other.
    ///
    /// This composes and stops. [`Self::show`] is what puts the result on a
    /// display; they are separate calls because a frame that was not redrawn
    /// still has to be *kept* on screen by some displays and not by others, and
    /// that is the display's business rather than the compositor's.
    pub fn compose(&mut self, compositor: &mut Compositor) -> bool {
        if !compositor.compose_frame() {
            return false;
        }
        self.stats.frames = self.stats.frames.saturating_add(1);
        true
    }

    /// Hand the last composited frame to a display.
    ///
    /// Uses [`Compositor::present_pixels`] rather than
    /// [`front_buffer`](Compositor::front_buffer): when the last frame was a
    /// fullscreen direct-scanout bypass the front buffer is stale, and showing
    /// it would put the previous frame on the screen — the one bug in this area
    /// that no pixel assertion in this crate would catch, because every such
    /// test reads the same stale buffer it asserts on.
    pub fn show<P: Present>(compositor: &Compositor, present: &mut P) {
        let (width, height) = compositor.frame_size();
        present.show(compositor.present_pixels(), width, height);
    }

    /// Serve clients and composite for ever, at the display's refresh rate.
    ///
    /// Equivalent to [`Self::run_with`] against [`Headless`] — the composited
    /// frame is produced and discarded, which is the correct behaviour for a
    /// compositor serving only remote clients, and the only behaviour available
    /// on a platform this crate cannot draw on.
    ///
    /// # Errors
    ///
    /// Only a failure of the listening socket, which ends the server. Every
    /// per-client failure ends that client instead.
    pub fn run(&mut self, compositor: &mut Compositor) -> io::Result<()> {
        self.run_with(compositor, &mut Headless)
    }

    /// Serve clients and composite for ever, onto `present`.
    ///
    /// The loop, in order: take whatever the user did and give it to the
    /// compositor, serve the clients (so that a click which raised a window is
    /// reflected in the events those clients are told about *this* frame, not
    /// next), composite, and show the result. Input first is the whole reason
    /// the ordering is written down here rather than left to look arbitrary.
    ///
    /// Returns when the display goes away — a closed host window — which is a
    /// normal end and not an error. A [`Headless`] display never goes away, so
    /// [`Self::run`] does not return.
    ///
    /// # Errors
    ///
    /// Only a failure of the listening socket, which ends the server. Every
    /// per-client failure ends that client instead.
    pub fn run_with<P: Present>(
        &mut self,
        compositor: &mut Compositor,
        present: &mut P,
    ) -> io::Result<()> {
        let interval = compositor
            .display_manager()
            .primary()
            .map_or(Duration::from_micros(16_667), Display::frame_interval);
        while present.is_open() {
            let began = Instant::now();
            for event in present.input() {
                compositor.handle_input(event);
            }
            self.tick(compositor)?;
            if self.compose(compositor) {
                Self::show(compositor, present);
            }
            // Whatever is left of the frame. Subtracting the work already done
            // rather than sleeping a flat interval, so a tick that took eight
            // milliseconds does not push the next frame to twenty-four.
            if let Some(rest) = interval.checked_sub(began.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use guiremote::client::Connection;
    use guiremote::control::{RequestBody, ResponseBody, WindowSpec};
    use guitk::color::Color;
    use guitk::render::RenderTree;

    use super::*;
    use crate::InputEvent;
    use crate::present::Recording;

    /// A server on a kernel-chosen port, and a compositor for it to drive.
    ///
    /// Port zero rather than a fixed one: these tests run beside each other and
    /// beside whatever else is on the machine.
    fn server() -> (Server, Compositor, SocketAddr) {
        let server = Server::bind("127.0.0.1:0").expect("bind");
        let addr = server.local_addr().expect("bound address");
        let compositor = Compositor::new(1920, 1080, 60).expect("compositor");
        (server, compositor, addr)
    }

    /// Dial the server and let it accept, returning the client's connection.
    fn dial(
        server: &mut Server,
        compositor: &mut Compositor,
        addr: SocketAddr,
    ) -> Connection<Socket> {
        let socket = Socket::connect(addr).expect("connect");
        for _ in 0..1000 {
            server.tick(compositor).expect("tick");
            if server.client_count() > 0 {
                return Connection::new(socket);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the server never accepted the connection");
    }

    /// Turn the crank until `seq` is answered.
    fn await_reply(
        server: &mut Server,
        compositor: &mut Compositor,
        conn: &mut Connection<Socket>,
        seq: u32,
    ) -> ResponseBody {
        for _ in 0..1000 {
            server.tick(compositor).expect("tick");
            conn.pump().expect("pump");
            if let Some(reply) = conn.take_reply(seq) {
                return reply;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the reply to {seq} never arrived");
    }

    #[test]
    fn a_client_can_connect_and_be_given_a_window() {
        // The whole point of the module, in one test: a real socket, the real
        // codecs on both sides, and a window that exists afterwards.
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);

        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Notes", 640, 480,
            )))
            .expect("send");
        let reply = await_reply(&mut server, &mut compositor, &mut conn, seq);

        let ResponseBody::WindowCreated { window } = reply else {
            panic!("expected a window, got {reply:?}");
        };
        assert_ne!(window, 0);
        assert_eq!(compositor.window_count(), 1);
        assert_eq!(server.stats().accepted, 1);
    }

    #[test]
    fn a_picture_a_client_submits_reaches_the_compositor() {
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);
        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Paint", 320, 240,
            )))
            .expect("send");
        let ResponseBody::WindowCreated { window } =
            await_reply(&mut server, &mut compositor, &mut conn, seq)
        else {
            panic!("no window");
        };

        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 100.0, 100.0, Color::BLUE);
        let mut frame = Vec::new();
        guiremote::submit::encode_submit_into(window, &tree, &mut frame);
        conn.transport_mut().write(&frame).expect("write");

        // A submission has no reply — that is the design, so that a repaint
        // does not cost a round trip — so the evidence is the served counter
        // moving, which is why it exists.
        let before = server.stats().served;
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.stats().served > before {
                assert!(compositor.compose_frame(), "the frame had nothing to draw");
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the submission never arrived");
    }

    #[test]
    fn a_departed_client_does_not_leave_its_windows_behind() {
        // A window outlives the link that opened it, so without this the
        // desktop accumulates windows no process can close, move or draw into
        // — one per application that ever crashed.
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);
        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Doomed", 200, 100,
            )))
            .expect("send");
        await_reply(&mut server, &mut compositor, &mut conn, seq);
        assert_eq!(compositor.window_count(), 1);

        drop(conn);
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.client_count() == 0 {
                assert_eq!(
                    compositor.window_count(),
                    0,
                    "the window outlived its client"
                );
                assert_eq!(server.stats().orphans_reclaimed, 1);
                assert_eq!(server.stats().disconnected, 1);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the disconnection was never noticed");
    }

    #[test]
    fn a_client_that_speaks_nonsense_is_dropped_and_the_rest_survive() {
        // One misbehaving application must not be able to stop the desktop.
        let (mut server, mut compositor, addr) = server();
        let mut good = dial(&mut server, &mut compositor, addr);
        let mut bad = Socket::connect(addr).expect("connect");
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.client_count() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            server.client_count(),
            2,
            "the second client never connected"
        );

        bad.write(b"NOPE\x02\x00\x01\x00\x00\x00").expect("write");
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.stats().protocol_errors > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            server.stats().protocol_errors,
            1,
            "the bad stream was tolerated"
        );
        assert_eq!(server.client_count(), 1, "the wrong client was dropped");

        // And the survivor is still served.
        let seq = good
            .send(RequestBody::CreateWindow(WindowSpec::new("Fine", 100, 100)))
            .expect("send");
        let reply = await_reply(&mut server, &mut compositor, &mut good, seq);
        assert!(matches!(reply, ResponseBody::WindowCreated { .. }));
    }

    #[test]
    fn a_client_cannot_touch_a_window_it_does_not_own() {
        // `ClientLink` enforces this and has its own tests; what this adds is
        // that the enforcement survives the trip through two real sockets,
        // which is where an ownership check wired to the wrong link would show
        // up.
        let (mut server, mut compositor, addr) = server();
        let mut owner = dial(&mut server, &mut compositor, addr);
        let seq = owner
            .send(RequestBody::CreateWindow(WindowSpec::new("Mine", 300, 200)))
            .expect("send");
        let ResponseBody::WindowCreated { window } =
            await_reply(&mut server, &mut compositor, &mut owner, seq)
        else {
            panic!("no window");
        };

        let intruder_socket = Socket::connect(addr).expect("connect");
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.client_count() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut intruder = Connection::new(intruder_socket);
        let seq = intruder
            .send(RequestBody::DestroyWindow { window })
            .expect("send");
        let reply = await_reply(&mut server, &mut compositor, &mut intruder, seq);
        assert!(
            matches!(reply, ResponseBody::Error { .. }),
            "a stranger closed someone else's window: {reply:?}"
        );
        assert_eq!(
            compositor.window_count(),
            1,
            "the window was destroyed anyway"
        );
    }

    #[test]
    fn every_connection_gets_its_own_identity() {
        // Two connections from one process are two clients. Routing or
        // ownership keyed on a shared id would send one window's keystrokes to
        // the other, which is a password-shaped bug rather than a cosmetic one.
        let (mut server, mut compositor, addr) = server();
        let _a = Socket::connect(addr).expect("connect");
        let _b = Socket::connect(addr).expect("connect");
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            if server.client_count() == 2 {
                assert_eq!(server.stats().accepted, 2);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("both connections never arrived");
    }

    #[test]
    fn an_idle_server_ticks_without_clients_or_error() {
        let (mut server, mut compositor, _addr) = server();
        for _ in 0..10 {
            server.tick(&mut compositor).expect("tick");
        }
        assert_eq!(server.client_count(), 0);
        assert_eq!(server.stats().accepted, 0);
    }

    #[test]
    fn a_composited_frame_reaches_the_display() {
        // The gap this closes, stated plainly: everything in this crate was
        // real up to the last step and then stopped. `compose_frame` blended a
        // desktop into a buffer and `front_buffer` handed out pixels nothing
        // looked at, so no test in the tree could tell a working compositor
        // from one that composited into a void.
        let (mut server, mut compositor, _addr) = server();
        compositor.create_window("Visible".to_owned(), 400, 300, 1);

        let mut screen = Recording::closing_after(4);
        server.run_with(&mut compositor, &mut screen).expect("run");

        assert_eq!(screen.ticks(), 4, "the loop ran exactly as long as told");
        assert!(screen.shown() > 0, "nothing ever reached the display");
        let (width, height, pixels) = screen.last_frame().expect("a frame");
        assert_eq!(
            (width, height),
            compositor.frame_size(),
            "the display was told the wrong shape for the pixels it was given"
        );
        assert_eq!(pixels.len(), width as usize * height as usize);
        assert!(
            server.stats().frames >= screen.shown(),
            "more frames were shown than were composed"
        );
    }

    #[test]
    fn what_the_user_does_at_the_display_reaches_the_compositor() {
        // The other half of the same gap, and it had the same cause: the hosted
        // build has no keyboard or mouse driver, so `handle_input` was reachable
        // from a test and from nothing else. A display that hands back input is
        // what connects a real device to it.
        let (mut server, mut compositor, _addr) = server();
        let before = compositor.cursor_position();

        let mut screen = Recording::closing_after(3);
        screen.feed(vec![InputEvent::MouseMove { x: 640, y: 400 }]);
        server.run_with(&mut compositor, &mut screen).expect("run");

        assert_ne!(before, (640, 400), "the test would prove nothing");
        assert_eq!(
            compositor.cursor_position(),
            (640, 400),
            "the pointer never moved, so the display's input went nowhere"
        );
    }

    #[test]
    fn a_display_that_is_already_closed_serves_nothing_and_returns() {
        // `run` is a loop that ends only when the display goes away, so the
        // degenerate case is worth pinning: it must return rather than
        // composing one last frame onto a screen that is not there.
        let (mut server, mut compositor, _addr) = server();
        let mut screen = Recording::new();
        screen.open = false;
        server.run_with(&mut compositor, &mut screen).expect("run");
        assert_eq!(screen.shown(), 0);
        assert_eq!(screen.ticks(), 0);
        assert_eq!(server.stats().frames, 0);
    }

    #[test]
    fn a_keystroke_at_the_display_is_routed_to_the_focused_client() {
        // End to end within one process: a real socket, a real window, a key
        // arriving the way a keyboard driver will deliver it, and the client
        // being told about it. Every link in that chain existed before this;
        // the first one had no far end.
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);
        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Typing", 640, 480,
            )))
            .expect("send");
        let ResponseBody::WindowCreated { window } =
            await_reply(&mut server, &mut compositor, &mut conn, seq)
        else {
            panic!("no window");
        };

        // 0x1E is `a` in scan code set 1 — the set both the keymap and the host
        // window speak, which is what lets a harness drive the real translation.
        let mut screen = Recording::new();
        screen.feed(vec![InputEvent::KeyDown {
            scancode: 0x1E,
            character: Some('a'),
        }]);
        screen.close_after = Some(2);
        server.run_with(&mut compositor, &mut screen).expect("run");

        assert!(
            server.stats().routed_events > 0,
            "the keystroke was not routed to anyone: {:?}",
            server.stats()
        );

        // And it reached the client, not merely the router. The scancode is the
        // one the display reported, unchanged: it is carried alongside the
        // translated key so that a game can ask for the physical position.
        for _ in 0..1000 {
            server.tick(&mut compositor).expect("tick");
            conn.pump().expect("pump");
            if conn
                .drain_events()
                .iter()
                .any(|e| e.window == window && e.scancode == Some(0x1E))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the client was never told about the keystroke");
    }
}
