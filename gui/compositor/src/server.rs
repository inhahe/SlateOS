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
//! ## How it waits
//!
//! Between ticks the loop *blocks*, on everything that could give it work at
//! once ([`guiremote::WaitSet`]): the listening socket, every client, the
//! display's input devices, and a deadline — the earliest of the next frame
//! that is owed, the display's own schedule (a hotplug probe, a key repeat) and
//! the compositor's (a slow key's threshold, a window's idle deadline). So a
//! request is read the moment it arrives rather than at the next frame, and a
//! desktop nobody is touching does not wake at all until one of those things
//! happens.
//!
//! It used to poll instead — wake once a frame and ask everything whether it
//! had anything — because the standard library has no way to wait on several
//! sockets at once. That put up to a frame of latency under every request and
//! woke an idle desktop sixty times a second to find nothing
//! (`known-issues.md` → `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`). `WaitSet`
//! is that missing primitive.
//!
//! **Frames are still paced.** A burst of requests or a fast mouse wakes the
//! loop as often as it likes, and every wake serves what arrived; but a frame
//! is shown at most once per display refresh, and one that is owed sooner
//! waits for its slot. Otherwise a 1000 Hz mouse would redraw the pointer a
//! thousand times a second on a sixty-hertz screen.
//!
//! **A wake knows what woke it.** A client the wait did not report is not read
//! on that tick, and the listener is not asked for connections nobody is
//! making. On SlateOS every socket call is a round trip to the network daemon,
//! so an event loop that read every client on every mouse movement would cost
//! more than the polling it replaced.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use guiremote::socket::{Listener, Socket};
use guiremote::{LISTENER_READINESS, WaitSet};
use inputsettings::InputSettings;

use appearance::ColorFilter;

use crate::present::{Frame, Headless, Present, earliest};
use crate::wire::ClientLink;
use crate::{Compositor, CursorCache, Display, PointerSprite, PointerState, WindowId};

/// What a shown frame's pixels were made from: the compositor's picture (by
/// its frame count), and the colour filter and night-light gains applied on
/// the way out. Two frames with the same key show the same pixels.
type ShownKey = (u64, ColorFilter, Option<(f32, f32, f32)>);

/// Whether the pointer is drawn over a fullscreen window being scanned out
/// directly.
///
/// `open-questions.md` C-Q18 asked the operator to choose between losing the
/// fullscreen shortcut, hiding the pointer over fullscreen, and a hardware
/// cursor, because a pointer painted *into* the frame cannot appear on a frame
/// that is never composited. The pointer is a layer drawn at presentation
/// instead (`crate::cursor`), and every presenter that exists already copies a
/// fullscreen window's pixels to the screen, so drawing it there costs nothing:
/// it is shown, which is what options A and C both give the user. A game that
/// wants no pointer asks for `CursorShape::Hidden` over its window.
///
/// `false` is C-Q18's option B. The choice returns in earnest when a presenter
/// scans a client's buffer out without copying it; see
/// `requests/f-c-c-q18s-premise-changed-the-pointer-is-drawn-over-fullscreen-at-no-cost.md`
/// and design-decisions §1301.
const POINTER_OVER_DIRECT_SCANOUT: bool = true;

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
    /// The client's incomplete frame passed `MAX_PENDING_INPUT`.
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
    /// Whether this tick should read the socket: `false` only when the wait
    /// before the tick found nothing on it. Put back to `true` once the tick
    /// has looked, so a caller driving [`Server::tick`] without waiting —
    /// every test, and every harness in the tree — reads everyone, as it
    /// always did.
    readable: bool,
}

/// The compositor's listening front end.
pub struct Server {
    listener: Listener,
    clients: Vec<Client>,
    /// Where a filtered frame is built, reused across frames.
    ///
    /// On the server rather than allocated per frame: at 1920x1080 this is
    /// eight megabytes, and allocating it sixty times a second to hand it
    /// straight to the display would cost more than the filter does. Empty
    /// until a filter is first switched on, so a user who never uses one
    /// never pays for it.
    filtered: Vec<u32>,
    /// What [`Self::filtered`] currently holds, so a frame that shows the same
    /// picture through the same filter reuses it. That is every frame in which
    /// only the pointer moved, and re-filtering eight megabytes for a pointer
    /// would cost more than drawing the pointer does.
    filtered_for: Option<ShownKey>,
    /// The rasterized pointers.
    ///
    /// Here rather than in the compositor, which only says *which* pointer is
    /// up ([`Compositor::pointer`]): what it looks like is decided on the way to
    /// the display, which is the step a hardware cursor plane would take over.
    cursors: CursorCache,
    /// The pointer the display was last shown, so the loop can tell that a
    /// frame is owed even though nothing was composed: the pointer moved.
    shown_pointer: Option<PointerState>,
    /// What the last frame shown was made from, and the serial it was given.
    /// The serial advances exactly when the key changes, which is what lets a
    /// presenter that keeps a copy of the picture skip re-copying it.
    shown: Option<(ShownKey, u64)>,
    /// When a frame was last handed to the display.
    ///
    /// The pacing clock: a frame is shown at most once per display refresh
    /// however often the loop wakes, and one owed sooner than that waits for
    /// its slot ([`Self::next_wake`]).
    last_shown: Option<Instant>,
    /// Whether this tick should ask the listener for connections: `false` only
    /// when the wait before it found none waiting. Same rule as
    /// `Client::readable`, for the same reason.
    listener_ready: bool,
    /// What the loop waits on between ticks, rebuilt before each wait and kept
    /// so that one wait allocates nothing after the first.
    waits: WaitSet,
    /// Whether the last wait failed, so a failure is reported when it starts
    /// and when it stops rather than sixty times a second in between.
    wait_failing: bool,
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
            filtered: Vec::new(),
            filtered_for: None,
            cursors: CursorCache::new(),
            shown_pointer: None,
            shown: None,
            last_shown: None,
            listener_ready: true,
            waits: WaitSet::new(),
            wait_failing: false,
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
    /// Reads every client and asks the listener for connections — unless the
    /// loop's wait has just said which of them have anything, in which case
    /// only those. That narrowing lasts one tick, so a caller that never waits
    /// always gets the whole round.
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
        // Before routing, so a deadline that passed during this tick reaches
        // its claimant in this tick's batch rather than waiting for the next.
        compositor.queue_idle_notifications(Instant::now());
        self.route_and_flush(compositor);
        self.reap(compositor);
        Ok(())
    }

    /// Take whatever connections are pending, up to the per-tick bound.
    fn accept_pending(&mut self) -> io::Result<()> {
        // The wait found nobody connecting, so asking would be a system call
        // (on SlateOS, a round trip to the network daemon) to be told so.
        if !std::mem::replace(&mut self.listener_ready, true) {
            return Ok(());
        }
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
                // A new connection may already have sent something; the wait
                // that found the listener ready knew nothing about this socket.
                readable: true,
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
            // Nothing arrived on it, so there is nothing to read, nothing new
            // to serve (`serve` acts on complete frames, and it completed every
            // one it had last time), and no hang-up to notice — a peer going
            // away is itself something the wait reports.
            if !std::mem::replace(&mut client.readable, true) {
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
    /// The colour-vision filter is applied here, and this is the only place
    /// it is applied. A filter is a transform of *finished* pixels, so the
    /// hand-off to the display is the one point that sees every pixel exactly
    /// once: composition writes only damaged rectangles, and a filter applied
    /// there would leave the undamaged remainder of the screen unfiltered.
    ///
    /// When no filter is set -- nearly always -- the frame is handed over
    /// untouched and nothing is copied.
    ///
    /// The pointer goes with it, as a layer over the picture rather than part
    /// of it ([`crate::cursor`]); it is filtered too, since a pointer that
    /// stayed cold white on a warmed screen would be the one thing the setting
    /// missed.
    pub fn show<P: Present>(&mut self, compositor: &Compositor, present: &mut P) {
        let (width, height) = compositor.frame_size();
        let pixels = compositor.present_pixels();
        let filter = compositor.color_filter();
        let warmth = compositor.night_light_gains();

        let pointer = Self::pointer_to_show(compositor);
        self.shown_pointer = pointer;
        self.last_shown = Some(Instant::now());
        let sprite = pointer.and_then(|state| self.cursors.sprite(&state));

        // The compositor's own count of the frames it has produced names the
        // picture, so the serial is right however `compose_frame` was reached.
        let key: ShownKey = (compositor.frame_stats().frames_composited, filter, warmth);
        let serial = match self.shown {
            Some((shown_key, serial)) if shown_key == key => serial,
            Some((_, serial)) => serial.wrapping_add(1),
            None => 0,
        };
        self.shown = Some((key, serial));

        if matches!(filter, ColorFilter::None) && warmth.is_none() {
            present.show(
                &Frame::new(pixels, width, height)
                    .with_serial(serial)
                    .with_pointer(sprite.as_ref()),
            );
            return;
        }

        // The accessibility filter first, then the warmth. The order is a
        // claim about what each one is: a colour-vision filter transforms the
        // *content*, so it should see the colours the application chose, while
        // night light is a property of the *display* -- the software stand-in
        // for a warm panel or a sheet of amber over the glass. Reversing them
        // would have a protanopia filter correcting for a tint the user added
        // on purpose, and hand back a screen that is neither warm nor
        // corrected.
        let shade = |p: u32| {
            let shown = filter.apply_argb(p);
            match warmth {
                Some(gains) => appearance::warm_argb(shown, gains),
                None => shown,
            }
        };
        if self.filtered_for != Some(key) || self.filtered.len() != pixels.len() {
            self.filtered.clear();
            self.filtered.reserve(pixels.len());
            self.filtered.extend(pixels.iter().map(|p| shade(*p)));
            self.filtered_for = Some(key);
        }
        let sprite = sprite.map(|sprite| PointerSprite {
            image: std::sync::Arc::new(sprite.image.filtered(shade)),
            ..sprite
        });
        present.show(
            &Frame::new(&self.filtered, width, height)
                .with_serial(serial)
                .with_pointer(sprite.as_ref()),
        );
    }

    /// Whether the display owes a frame even though nothing was composed:
    /// the pointer has moved, changed shape, or come or gone since the last
    /// frame shown.
    #[must_use]
    pub fn pointer_changed(&self, compositor: &Compositor) -> bool {
        Self::pointer_to_show(compositor) != self.shown_pointer
    }

    /// The pointer the next frame should carry: the compositor's, unless the
    /// frame is a direct scanout and [`POINTER_OVER_DIRECT_SCANOUT`] says not
    /// to draw one there. One function for both [`Self::show`] and
    /// [`Self::pointer_changed`], so the two cannot disagree about whether a
    /// frame is owed.
    fn pointer_to_show(compositor: &Compositor) -> Option<PointerState> {
        if !POINTER_OVER_DIRECT_SCANOUT && compositor.is_scanout_bypassed() {
            return None;
        }
        compositor.pointer()
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

    /// Bring the compositor's set of monitors into line with the display's.
    ///
    /// The *propagate* half of monitor hotplug. [`Present::monitors`] reports
    /// what the scanout is actually driving; the compositor holds the
    /// arrangement that places windows and answers "which screen is this on?".
    /// Nothing else joins the two, so without this a monitor plugged in stays
    /// dark and one unplugged keeps windows on a screen that is gone.
    ///
    /// Reconciled **by id**, and only by id — the sizes and offsets are not
    /// compared. A monitor whose *mode* changed under a live connector is
    /// `TD-COMPOSITOR-CANNOT-CHANGE-MODE` and is a different problem with a
    /// different fix ([`Compositor::resize_display`]); treating it here by
    /// detaching and re-attaching would destroy the window arrangement on that
    /// screen to achieve a resize the compositor can already do in place.
    ///
    /// **Removals before additions**, for two reasons that point the same way:
    /// the peak surface size over the operation is the smaller one rather than
    /// the union, and a monitor swapped for another in the same slot does not
    /// transiently need the framebuffer for both.
    ///
    /// Failures are reported and dropped rather than propagated. A monitor the
    /// compositor cannot paint for is worse than one it does not know about —
    /// the scanout would go on copying out a rectangle of frame that was never
    /// composed — but the alternative to carrying on is bringing the whole
    /// display server down because a screen was plugged in, and the poll is
    /// idempotent, so the next tick tries again.
    fn reconcile_monitors<P: Present>(compositor: &mut Compositor, present: &mut P) {
        let Some(monitors) = present.monitors() else {
            return;
        };
        if monitors.is_empty() {
            // Not an arrangement — see `Present::monitors`. A display with no
            // monitors left is expected to close, and detaching every display to
            // match would leave a compositor with a zero-sized desktop.
            return;
        }

        let departed: Vec<u32> = compositor
            .display_manager()
            .displays()
            .iter()
            .map(|d| d.id)
            .filter(|id| !monitors.iter().any(|m| m.id == *id))
            .collect();
        for id in departed {
            if let Err(e) = compositor.detach_display(id) {
                eprintln!("compositor: monitor {id} was unplugged and cannot be dropped: {e}");
            }
        }

        for monitor in &monitors {
            if compositor
                .display_manager()
                .displays()
                .iter()
                .any(|d| d.id == monitor.id)
            {
                continue;
            }
            // Never primary: an arriving monitor joins a desktop that already
            // has one, and promoting it would move every rule-placed window on
            // the machine because a screen was plugged in.
            let display = Display::new(
                monitor.id,
                monitor.width,
                monitor.height,
                monitor.refresh_hz,
                1.0,
                false,
            );
            if let Err(e) = compositor.attach_display(display) {
                eprintln!(
                    "compositor: monitor {} was plugged in and cannot be composited for ({e}); it will stay dark",
                    monitor.id
                );
            }
        }
    }

    /// Push the user's input preferences into the display, if they have moved.
    ///
    /// The compositor reads `input.yaml` and applies the one setting it is
    /// itself the consumer of (the double-click window); everything else —
    /// pointer speed, acceleration, button mapping, key repeat — has to reach
    /// the thing that turns raw device deltas into a pointer position and a
    /// repeat, which is the input source behind `present`. This is the wire
    /// between the two.
    ///
    /// **Polled, not pushed**, exactly like [`Self::reconcile_monitors`] and
    /// for the same reason: a push would need a queue between the request
    /// handler and the loop, and a queue is a thing that can get out of step
    /// with what it describes. Once a tick, ask what the settings are; if they
    /// are not what was last handed over, hand them over.
    ///
    /// `pushed` is the loop's memory of what it last sent, and comparing
    /// against it is what keeps this to an equality test per frame rather than
    /// a call into the pointer sixty times a second. It starts as `None`, and
    /// so does [`Compositor::input_settings`] until the file has been read —
    /// which is the case that must *not* push, because the input source was
    /// constructed with the user's real settings and a "default" push would
    /// undo them.
    fn reconcile_input<P: Present>(
        compositor: &Compositor,
        present: &mut P,
        pushed: &mut Option<InputSettings>,
    ) {
        let Some(settings) = compositor.input_settings() else {
            return;
        };
        if pushed.as_ref() == Some(settings) {
            return;
        }
        present.reload_input(settings);
        *pushed = Some(settings.clone());
    }

    /// Serve clients and composite for ever, onto `present`.
    ///
    /// The loop, in order: take whatever the user did and give it to the
    /// compositor, serve the clients (so that a click which raised a window is
    /// reflected in the events those clients are told about *this* frame, not
    /// next), composite and show if a frame is owed and its slot has come, and
    /// then wait — for a client, the user, or the next thing due. Input first
    /// is the whole reason the ordering is written down here rather than left
    /// to look arbitrary.
    ///
    /// Monitors are reconciled before any of it, so that a frame is never
    /// composed for an arrangement the display has already stopped driving.
    ///
    /// Returns when the display goes away — a closed host window — which is a
    /// normal end and not an error. A [`Headless`] display never goes away, so
    /// [`Self::run`] does not return.
    ///
    /// # Errors
    ///
    /// Only a failure of the listening socket, which ends the server. Every
    /// per-client failure ends that client instead, and a wait that fails is
    /// reported and replaced by sleeping, not treated as fatal.
    pub fn run_with<P: Present>(
        &mut self,
        compositor: &mut Compositor,
        present: &mut P,
    ) -> io::Result<()> {
        let interval = compositor
            .display_manager()
            .primary()
            .map_or(Duration::from_micros(16_667), Display::frame_interval);
        // What was last handed to the input source. Lives here rather than on
        // `Server` because it describes this display, and a `Server` outlives
        // the `Present` it is run with.
        let mut pushed_input: Option<InputSettings> = None;
        while present.is_open() {
            Self::reconcile_monitors(compositor, present);
            // Before the poll, not after: the events this tick returns were
            // scaled by whatever the source is holding when it is asked, so
            // pushing afterwards would spend one frame moving the pointer at
            // the old speed after the user let go of the slider.
            Self::reconcile_input(compositor, present, &mut pushed_input);
            for event in present.input() {
                compositor.handle_input(event);
            }
            // A keystroke held back by slow keys is delivered here, once its
            // threshold has expired with the key still down
            // (`design-decisions.md` §821). The loop wakes for exactly that
            // moment: it is one of the deadlines `Compositor::wake_at` reports.
            compositor.poll_deferred_key();
            self.tick(compositor)?;
            self.present_if_due(compositor, present, interval);

            // Checked again before waiting, not only at the top: a display
            // that closed during this tick — the close button, a recording
            // that has seen what it came for — has nothing left to wake for,
            // and a wait for it could last for ever.
            if !present.is_open() {
                break;
            }
            let now = Instant::now();
            let wake = earliest(
                self.next_wake(compositor, present.deadline(), interval, now),
                accept_deadline(LISTENER_READINESS, now, interval),
            );
            self.wait_for_work(present, wake, interval);
        }
        Ok(())
    }

    /// Compose and show a frame, if one is owed and its slot has come.
    ///
    /// A frame is owed when there is something to draw or the pointer has
    /// moved; its slot comes one refresh after the last frame shown. A frame
    /// owed before its slot is left for [`Self::next_wake`] to wake the loop
    /// for.
    fn present_if_due<P: Present>(
        &mut self,
        compositor: &mut Compositor,
        present: &mut P,
        interval: Duration,
    ) {
        let now = Instant::now();
        let slot_open = self
            .last_shown
            .is_none_or(|at| now.saturating_duration_since(at) >= interval);
        if !slot_open {
            return;
        }
        let composed = self.compose(compositor);
        // A pointer that moved over a still desktop is a frame too: the
        // picture is unchanged, and the presenter repaints only where the
        // pointer was and is.
        if composed || self.pointer_changed(compositor) {
            self.show(compositor, present);
        }
    }

    /// When the loop must run next if nothing arrives first, or `None` if
    /// nothing is due at all.
    ///
    /// The earliest of:
    ///
    /// * `display` — the display's own schedule ([`Present::deadline`]);
    /// * the compositor's ([`Compositor::wake_at`]): a slow key's threshold, a
    ///   window's idle deadline;
    /// * the next frame, if one is owed — damage to draw or a pointer that
    ///   moved — at its slot: one refresh (`interval`) after the last frame
    ///   shown, and not before the compositor's own frame clock allows
    ///   ([`crate::FrameStats::next_compose_at`]). `now` stands in for the slot
    ///   when nothing has been shown yet.
    ///
    /// Public, and taking the time as an argument, so that the rule can be
    /// asserted on directly: a loop that ends only when its window closes is
    /// not a thing a test can watch deciding when to wake.
    #[must_use]
    pub fn next_wake(
        &self,
        compositor: &Compositor,
        display: Option<Instant>,
        interval: Duration,
        now: Instant,
    ) -> Option<Instant> {
        let mut wake = earliest(display, compositor.wake_at());
        let damage = compositor.frame_owed();
        if damage || self.pointer_changed(compositor) {
            let mut due = self
                .last_shown
                .and_then(|at| at.checked_add(interval))
                .unwrap_or(now);
            if damage && let Some(ready) = compositor.frame_stats().next_compose_at() {
                // The later of the two: waking at the slot to find the
                // compositor's clock not yet willing would only mean waking
                // again, a moment later, with nothing done in between.
                due = due.max(ready);
            }
            wake = earliest(wake, Some(due));
        }
        wake
    }

    /// Block until a client or the display has something, or `wake` comes.
    ///
    /// Notes what the wait found, so the tick after it reads only what has
    /// something to read. A wait that fails — which would be a fault in the
    /// platform's machinery, not in any client — is reported once and replaced
    /// by sleeping to the deadline or for a frame, whichever is sooner, with
    /// everything left to be read: the loop degrades to polling rather than
    /// stopping the desktop or spinning.
    fn wait_for_work<P: Present>(
        &mut self,
        present: &mut P,
        wake: Option<Instant>,
        interval: Duration,
    ) {
        self.waits.clear();
        let listener = self.waits.add_source(&self.listener);
        let first_client = self.waits.len();
        for client in &self.clients {
            self.waits.add_source(&client.socket);
        }
        present.wait_on(&mut self.waits);

        let timeout = wake.map(|at| at.saturating_duration_since(Instant::now()));
        match present.wait(&mut self.waits, timeout) {
            Ok(()) => {
                if self.wait_failing {
                    eprintln!("compositor: waiting for work succeeds again");
                    self.wait_failing = false;
                }
                self.listener_ready =
                    listener_worth_asking(LISTENER_READINESS, self.waits.is_ready(listener));
                for (offset, client) in self.clients.iter_mut().enumerate() {
                    client.readable = self.waits.is_ready(first_client.saturating_add(offset));
                }
            }
            Err(e) => {
                if !self.wait_failing {
                    eprintln!(
                        "compositor: cannot wait for work ({e}); looking for it every frame instead"
                    );
                    self.wait_failing = true;
                }
                std::thread::sleep(timeout.map_or(interval, |t| t.min(interval)));
            }
        }
    }
}

/// Whether the tick after a wait should ask the listener for connections.
///
/// Only when the wait said one is waiting — where the platform can say so at
/// all (`reported`, which is [`LISTENER_READINESS`]). Where it cannot, silence
/// from the listener means nothing, and asking every tick is the only way to
/// learn anyone connected.
const fn listener_worth_asking(reported: bool, ready: bool) -> bool {
    !reported || ready
}

/// The latest the loop may wait before asking the listener again, where the
/// platform will not wake it for a connection (`reported` false): one frame,
/// which is how long a program starting up waited to be accepted before the
/// loop learned to wait at all. `None` where a connection wakes the loop by
/// itself.
fn accept_deadline(reported: bool, now: Instant, interval: Duration) -> Option<Instant> {
    if reported {
        None
    } else {
        now.checked_add(interval)
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
    use crate::AppearanceSettings;
    use crate::InputEvent;
    use crate::present::{MonitorInfo, Recording};

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
        // The sibling counter, which nothing asserted until 2026-09-11 and
        // which is the one with a meaningful threshold. `routed_events > 0`
        // only says somebody got something; this says nothing fell on the
        // floor. An event no live link claims is discarded here precisely so it
        // cannot accumulate and later be delivered to whoever next opens a
        // window with a recycled id -- so a non-zero count is a real defect and
        // not a slow path.
        //
        // Deliberately not pinning `routed_events` to an exact number. It
        // measured 2 for this one keystroke, but `route_and_flush` runs per
        // tick and the harness closes after two, so the figure is a property of
        // the fixture rather than of the routing. A bound whose value a later
        // reader cannot explain is worse than none: it gets relaxed rather than
        // investigated the first time it fires.
        assert_eq!(
            server.stats().unrouted_events,
            0,
            "an input event reached nobody: {:?}",
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

    // ---- a monitor plugged in or unplugged reaches the compositor ----

    fn monitor(id: u32, width: u32, height: u32) -> MonitorInfo {
        MonitorInfo {
            id,
            width,
            height,
            refresh_hz: 60,
        }
    }

    #[test]
    fn a_display_with_no_opinion_about_monitors_leaves_the_arrangement_alone() {
        // The default, and what every test written before hotplug relies on: a
        // host window and a headless server have no connectors to enumerate, and
        // a reconciliation that treated "I do not know" as "there are none"
        // would tear down the desktop of every one of them.
        let (mut server, mut compositor, _addr) = server();
        compositor
            .attach_display(Display::new(7, 1024, 768, 60, 1.0, false))
            .expect("attach");
        let before: Vec<u32> = compositor
            .display_manager()
            .displays()
            .iter()
            .map(|d| d.id)
            .collect();

        let mut screen = Recording::closing_after(3);
        assert_eq!(screen.monitors, None, "the default is no opinion");
        server.run_with(&mut compositor, &mut screen).expect("run");

        let after: Vec<u32> = compositor
            .display_manager()
            .displays()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(
            after, before,
            "a display with no opinion changed the desktop"
        );
    }

    #[test]
    fn a_monitor_plugged_in_joins_the_desktop() {
        // The bug this closes: `DrmScanout::new` enumerates connectors once, so
        // a screen plugged in afterwards stayed dark until the display server
        // was restarted. Nothing here knew a connector could change.
        let (mut server, mut compositor, _addr) = server();
        assert_eq!(compositor.frame_size(), (1920, 1080));

        let mut screen = Recording::closing_after(3);
        screen.monitors = Some(vec![monitor(0, 1920, 1080), monitor(31, 1024, 768)]);
        server.run_with(&mut compositor, &mut screen).expect("run");

        let displays = compositor.display_manager().displays();
        assert_eq!(displays.len(), 2, "the new monitor never arrived");
        assert_eq!(displays[1].id, 31, "it arrived under the wrong identity");
        assert_eq!((displays[1].width, displays[1].height), (1024, 768));
        assert!(
            !displays[1].primary,
            "an arriving monitor took over as primary"
        );
        assert_eq!(
            compositor.frame_size(),
            (2944, 1080),
            "the composited surface does not reach the new monitor, so it would \
             scan out pixels that were never composed"
        );
    }

    #[test]
    fn a_monitor_unplugged_leaves_the_desktop_and_takes_its_windows_with_it() {
        let (mut server, mut compositor, _addr) = server();
        compositor
            .attach_display(Display::new(31, 1024, 768, 60, 1.0, false))
            .expect("attach");
        let window = compositor.create_window("Over there".to_owned(), 400, 300, 1);
        compositor.move_window(window, 2000, 100).expect("move");
        compositor.maximize_window(window).expect("maximize");

        let mut screen = Recording::closing_after(3);
        screen.monitors = Some(vec![monitor(0, 1920, 1080)]);
        server.run_with(&mut compositor, &mut screen).expect("run");

        assert_eq!(
            compositor.display_manager().displays().len(),
            1,
            "the unplugged monitor is still on the desktop"
        );
        assert_eq!(compositor.frame_size(), (1920, 1080));
        let framed = compositor.window_ref(window).expect("window").frame_rect();
        assert!(
            framed.x < 1920,
            "a maximised window was left on the monitor that was unplugged, \
             where it has no title bar on any surviving screen to be dragged \
             back by: {framed:?}"
        );
    }

    #[test]
    fn a_monitor_that_only_changed_mode_is_not_torn_down_and_rebuilt() {
        // Reconciliation is by id and only by id. A connector still plugged in
        // but running a different mode is TD-COMPOSITOR-CANNOT-CHANGE-MODE, and
        // detaching and re-attaching it to achieve the resize would throw away
        // the window arrangement on that screen -- and, because `add_display`
        // appends on the right, move the monitor itself to the end of the row.
        let (mut server, mut compositor, _addr) = server();
        compositor
            .attach_display(Display::new(31, 1024, 768, 60, 1.0, false))
            .expect("attach");

        let mut screen = Recording::closing_after(3);
        screen.monitors = Some(vec![monitor(0, 1920, 1080), monitor(31, 640, 480)]);
        server.run_with(&mut compositor, &mut screen).expect("run");

        let displays = compositor.display_manager().displays();
        assert_eq!(
            displays.len(),
            2,
            "the monitor was detached over a mode change"
        );
        assert_eq!(
            (displays[1].width, displays[1].height),
            (1024, 768),
            "the mode change was applied here, where it would have destroyed the \
             screen's window arrangement to do it"
        );
    }

    #[test]
    fn an_empty_monitor_list_is_not_an_arrangement_to_adopt() {
        // A display that has lost every monitor is expected to close, and
        // `is_open` is what says so. Reconciling to nothing would leave a
        // compositor with a zero-sized desktop and every window on no screen.
        //
        // Two monitors, deliberately. With one, `detach_display` refuses the
        // last one anyway and the desktop survives a missing guard by accident
        // -- which would make this test agree with a broken reconciliation.
        let (mut server, mut compositor, _addr) = server();
        compositor
            .attach_display(Display::new(31, 1024, 768, 60, 1.0, false))
            .expect("attach");

        let mut screen = Recording::closing_after(3);
        screen.monitors = Some(Vec::new());
        server.run_with(&mut compositor, &mut screen).expect("run");
        assert_eq!(
            compositor.display_manager().displays().len(),
            2,
            "the desktop was reconciled down towards no monitors at all"
        );
        assert_eq!(compositor.frame_size(), (2944, 1080));
    }

    #[test]
    fn reconciling_the_same_set_twice_changes_nothing_the_second_time() {
        // The whole reason the poll reports a set rather than a list of changes:
        // asking twice has to give the same answer, so a tick that failed or was
        // skipped is simply retried. A reconciliation that acted on the *reply*
        // rather than on the difference would re-attach the same monitor on
        // every one of the sixty ticks a second this runs at.
        let (mut server, mut compositor, _addr) = server();
        let mut screen = Recording::closing_after(8);
        screen.monitors = Some(vec![monitor(0, 1920, 1080), monitor(31, 1024, 768)]);
        server.run_with(&mut compositor, &mut screen).expect("run");
        assert_eq!(
            compositor.display_manager().displays().len(),
            2,
            "eight ticks of the same two monitors produced something other than \
             two monitors"
        );
        assert_eq!(compositor.frame_size(), (2944, 1080));
    }

    // -----------------------------------------------------------------------
    // Carrying the user's input preferences to the device
    // -----------------------------------------------------------------------

    /// A display that records what it is told about input, and in what order.
    ///
    /// A whole [`Present`] rather than a [`Paired`] with a listening
    /// [`InputSource`](crate::present::InputSource) in it, for two reasons.
    /// What is under test here is the loop's own behaviour — when it pushes and
    /// when it does not — and the loop only ever sees a `Present`; that
    /// `Paired` hands the call on to its source is proved where `Paired` lives.
    /// And a `Paired<Recording, _>` cannot end a `run_with` at all:
    /// `Recording::closing_after` counts calls to `Recording::input`, which a
    /// pairing never makes, because it polls the source instead.
    #[derive(Debug, Default)]
    struct Listening {
        /// Every settings pushed into it, in order.
        reloads: Vec<InputSettings>,
        /// `"reload"` and `"poll"`, in the order they happened.
        log: Vec<&'static str>,
        /// Ticks so far.
        ticks: u64,
        /// Ticks to stay open for.
        limit: u64,
    }

    impl Present for Listening {
        fn show(&mut self, _frame: &Frame<'_>) {}

        fn input(&mut self) -> Vec<InputEvent> {
            self.ticks = self.ticks.saturating_add(1);
            self.log.push("poll");
            Vec::new()
        }

        fn is_open(&self) -> bool {
            self.ticks < self.limit
        }

        fn reload_input(&mut self, settings: &InputSettings) {
            self.log.push("reload");
            self.reloads.push(settings.clone());
        }

        /// Always due: this display ends the loop by counting ticks, and a
        /// count of ticks only advances if the loop keeps ticking.
        fn deadline(&self) -> Option<Instant> {
            Some(Instant::now())
        }
    }

    /// A listening display that ends the loop after `ticks` ticks.
    fn listening_display(ticks: u64) -> Listening {
        Listening {
            limit: ticks,
            ..Listening::default()
        }
    }

    #[test]
    fn the_pointer_speed_the_user_chose_reaches_the_device_without_a_relogin() {
        // The bug this closes: `Compositor::reload_input` read `input.yaml`,
        // kept the double-click window and threw the rest away, so the Settings
        // panel's speed slider wrote a number that nothing read until the next
        // login. The control appeared not to work.
        let (mut server, mut compositor, _addr) = server();
        let mut settings = InputSettings::default();
        settings.mouse.speed = 6;
        compositor.set_input_settings(settings);

        let mut display = listening_display(3);
        server.run_with(&mut compositor, &mut display).expect("run");

        assert_eq!(display.reloads.len(), 1, "three ticks, one push");
        assert_eq!(
            display.reloads[0].mouse.speed, 6,
            "and it carried the user's speed"
        );
    }

    #[test]
    fn a_settings_change_mid_session_is_pushed_and_an_unchanged_one_is_not() {
        // The push is on the *difference*, not on the reply, for exactly the
        // reason the monitor poll is: this runs sixty times a second, and a
        // push per tick would be a call into the pointer for every frame the
        // user did not touch the Settings panel. Driven a tick at a time rather
        // than through `run_with`, because the interesting moment is a change
        // that happens *between* two ticks of one session.
        let (_server, mut compositor, _addr) = server();
        compositor.set_input_settings(InputSettings::default());
        let mut display = listening_display(0);
        let mut pushed = None;

        for _ in 0..4 {
            Server::reconcile_input(&compositor, &mut display, &mut pushed);
        }
        assert_eq!(display.reloads.len(), 1, "four ticks, one push");

        // The user drags the slider and Settings sends `ReloadInput`.
        let mut faster = InputSettings::default();
        faster.mouse.speed = -3;
        compositor.set_input_settings(faster);
        for _ in 0..4 {
            Server::reconcile_input(&compositor, &mut display, &mut pushed);
        }

        assert_eq!(
            display.reloads.len(),
            2,
            "the change never reached the pointer"
        );
        assert_eq!(
            display.reloads[1].mouse.speed, -3,
            "or reached it with the old value"
        );
    }

    #[test]
    fn a_compositor_that_has_not_read_the_file_pushes_nothing_at_all() {
        // `None` is not the defaults, and this is why. The input source is
        // built with the user's real settings already in hand; if "not read
        // yet" were spelled as `InputSettings::default()`, the first tick of
        // every session would push stock settings over them and the pointer
        // would revert to default speed until somebody edited the file.
        let (mut server, mut compositor, _addr) = server();
        assert_eq!(compositor.input_settings(), None, "nothing was loaded");

        let mut display = listening_display(3);
        server.run_with(&mut compositor, &mut display).expect("run");

        assert!(
            display.reloads.is_empty(),
            "a compositor that knows nothing overwrote what the device knew"
        );
    }

    #[test]
    fn the_settings_reach_the_device_before_the_first_event_is_polled() {
        // Ordering, and not a detail: the events a poll returns were already
        // scaled by whatever the source was holding when it was asked. Pushing
        // after the poll would spend a frame moving the pointer at the old
        // speed after the user let go of the slider — visible as a control
        // that lags one frame behind itself.
        let (mut server, mut compositor, _addr) = server();
        compositor.set_input_settings(InputSettings::default());

        let mut display = listening_display(2);
        server.run_with(&mut compositor, &mut display).expect("run");

        assert_eq!(
            display.log,
            vec!["reload", "poll", "poll"],
            "the device was told after it had already answered"
        );
    }

    // ------------------------------------------------------------------
    // Waiting for work
    //
    // The loop blocks between ticks, and these check both halves of that: it
    // wakes for everything it should — a client, the display, a deadline —
    // and for nothing else. The second half is the one a timer loop gets
    // wrong without any test noticing, since a loop that wakes too often is
    // still a correct loop, only a wasteful one.
    // ------------------------------------------------------------------

    const FRAME: Duration = Duration::from_micros(16_667);

    /// A desktop nobody is touching does not wake: after its first frame, the
    /// loop sleeps until the display goes away. The timer loop this replaced
    /// woke eighteen times in the same 300 ms.
    #[test]
    fn an_idle_desktop_does_not_wake_until_something_happens() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        display.close_at = Some(Instant::now() + Duration::from_millis(300));
        server.run_with(&mut compositor, &mut display).expect("run");

        assert_eq!(display.shown(), 1, "the first frame, and nothing after it");
        assert!(
            display.ticks() <= 3,
            "the loop woke {} times on a desktop where nothing happened",
            display.ticks()
        );
    }

    /// A client's request wakes the loop and is answered at once, rather than
    /// at the next frame — and the loop does not otherwise tick.
    ///
    /// Nothing here bounds *how fast* the answer comes, which a loaded machine
    /// decides. The loop's only timer is a watchdog a minute away, so an answer
    /// that arrives at all, before it, arrived because the request woke the
    /// loop; and the tick count, which load cannot raise, says the loop did
    /// not tick for any other reason.
    #[test]
    fn a_request_wakes_the_loop_and_is_answered_without_a_timer() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let watchdog = Instant::now() + Duration::from_mins(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_loop = Arc::clone(&stop);
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        // Built on the loop's own thread, as the shipped binary builds it: the
        // loop owns both for as long as it runs.
        let loop_thread = std::thread::spawn(move || {
            let (mut server, mut compositor, addr) = server();
            addr_tx.send(addr).expect("the test is waiting for this");
            let mut display = Recording::new();
            display.close_at = Some(watchdog);
            display.stop = Some(stop_for_loop);
            server.run_with(&mut compositor, &mut display).expect("run");
            (*server.stats(), display.ticks())
        });
        let addr = addr_rx.recv().expect("the loop started");

        let mut conn = Connection::new(Socket::connect(addr).expect("connect"));
        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Quick", 200, 100,
            )))
            .expect("send");
        let reply = loop {
            conn.pump().expect("pump");
            if let Some(reply) = conn.take_reply(seq) {
                break reply;
            }
            assert!(
                Instant::now() < watchdog,
                "the request was never answered: the loop did not wake for it"
            );
            conn.transport_mut()
                .set_wait_timeout(Some(Duration::from_millis(50)))
                .expect("a non-zero timeout");
            conn.transport_mut().wait().expect("wait");
        };
        // Hanging up is what wakes the loop to notice it has been stopped.
        stop.store(true, Ordering::Release);
        drop(conn);
        let (stats, ticks) = loop_thread.join().expect("the loop");

        assert!(
            matches!(reply, ResponseBody::WindowCreated { .. }),
            "{reply:?}"
        );
        assert_eq!(stats.accepted, 1);
        assert!(stats.served >= 1);
        // Connecting, the request, the window's first frame and the hang-up
        // are a handful of wakes; a timer loop ticks sixty times a second for
        // as long as this ran.
        assert!(ticks < 30, "the loop ticked {ticks} times for one request");
    }

    /// A pointer that moves just after a frame is shown when its slot comes,
    /// without anything else happening to wake the loop.
    #[test]
    fn the_loop_shows_a_frame_when_only_the_pointer_moved() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        display.feed(Vec::new());
        display.feed(vec![InputEvent::MouseMove { x: 30, y: 40 }]);
        display.close_once_shown = Some(2);
        // Only reached if the pointer's frame is never shown.
        display.close_at = Some(Instant::now() + Duration::from_mins(1));
        server
            .run_with(&mut compositor, &mut display)
            .expect("the loop");
        assert_eq!(
            display.shown(),
            2,
            "the first frame, then one for the pointer"
        );
        let pointer = display.last_pointer().expect("a pointer");
        assert_eq!(
            pointer.x + i32::try_from(pointer.image.hot_x).unwrap(),
            30,
            "the frame shown was not the one with the pointer moved"
        );
    }

    /// A mouse far faster than the display is drawn once per refresh, at
    /// wherever it has got to: every movement is taken in, and the frames are
    /// not multiplied to match.
    ///
    /// The frame count is bounded by the refreshes that actually elapsed,
    /// measured, so a slow machine is allowed its extra frames and cannot fail
    /// this; a loop that drew a frame per movement fails it on any machine
    /// that handles forty movements in less than twenty refreshes.
    #[test]
    fn a_fast_pointer_is_shown_once_per_refresh_at_its_latest_position() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        let moves = 40;
        for i in 1..=moves {
            display.feed(vec![InputEvent::MouseMove { x: i * 10, y: 50 }]);
        }
        // Ends when the loop has taken in every movement and shown whatever it
        // owed for them; the watchdog is only for a loop that never settles.
        display.close_when_idle = true;
        display.close_at = Some(Instant::now() + Duration::from_mins(1));
        let began = Instant::now();
        server
            .run_with(&mut compositor, &mut display)
            .expect("the loop");
        let took = began.elapsed();

        let pointer = display.last_pointer().expect("a pointer");
        assert_eq!(
            pointer.x + i32::try_from(pointer.image.hot_x).unwrap(),
            moves * 10,
            "the last frame did not show where the pointer ended up"
        );
        // One frame at the start, then at most one per refresh since, and one
        // more for the interval being a hair under `FRAME` at 60 Hz.
        let refreshes = took.as_nanos() / FRAME.as_nanos();
        assert!(
            u128::from(display.shown()) <= refreshes + 2,
            "{} frames in {took:?} ({refreshes} refreshes): the pointer is not paced to the display",
            display.shown()
        );
    }

    /// Nothing owed, nothing scheduled: no reason to wake at all.
    #[test]
    fn with_nothing_owed_the_loop_has_no_deadline() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        assert!(server.compose(&mut compositor));
        server.show(&compositor, &mut display);
        let now = Instant::now();
        assert_eq!(server.next_wake(&compositor, None, FRAME, now), None);

        let later = now + Duration::from_secs(1);
        assert_eq!(
            server.next_wake(&compositor, Some(later), FRAME, now),
            Some(later),
            "the display's own schedule is passed through"
        );
    }

    /// Something to draw is due one refresh after the last frame shown.
    #[test]
    fn damage_is_due_at_the_next_frame_slot() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        assert!(server.compose(&mut compositor));
        server.show(&compositor, &mut display);
        let shown_at = server.last_shown.expect("a frame was shown");

        compositor.create_window("Fresh".to_owned(), 300, 200, 1);
        assert!(
            compositor.frame_owed(),
            "the test premise: a window to draw"
        );
        let wake = server
            .next_wake(&compositor, None, FRAME, Instant::now())
            .expect("a frame is owed");
        assert!(
            wake >= shown_at + FRAME,
            "due before its slot: {:?}",
            wake - shown_at
        );
        assert!(
            wake <= shown_at + FRAME + Duration::from_millis(1),
            "due long after its slot: {:?}",
            wake - shown_at
        );
    }

    /// A pointer that moved is owed a frame too, at the same slot.
    #[test]
    fn a_moved_pointer_is_due_at_the_next_frame_slot() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        assert!(server.compose(&mut compositor));
        server.show(&compositor, &mut display);
        let shown_at = server.last_shown.expect("a frame was shown");

        compositor.handle_input(InputEvent::MouseMove { x: 5, y: 6 });
        assert!(!compositor.frame_owed(), "a pointer move is not damage");
        assert_eq!(
            server.next_wake(&compositor, None, FRAME, Instant::now()),
            Some(shown_at + FRAME)
        );
    }

    /// Before any frame has been shown, an owed one is due now.
    #[test]
    fn the_first_frame_is_due_at_once() {
        let (server, compositor, _addr) = server();
        assert!(compositor.frame_owed(), "a new desktop has to be drawn");
        let now = Instant::now();
        assert_eq!(server.next_wake(&compositor, None, FRAME, now), Some(now));
    }

    /// A keystroke waiting out its slow-keys threshold is a deadline the loop
    /// wakes for, not a reason to keep ticking — and nothing else would wake
    /// it, because to a loop waiting for input a held-back key is silence.
    #[test]
    fn a_keystroke_waiting_on_its_threshold_is_a_deadline() {
        let mut comp = Compositor::new(320, 240, 60).unwrap();
        comp.create_window("Editor".to_string(), 200, 150, 1);
        comp.set_accessibility_keys(inputsettings::AccessibilityKeysConfig {
            filter: inputsettings::FilterKeysConfig {
                enabled: true,
                slow_keys_ms: 300,
                ..inputsettings::FilterKeysConfig::default()
            },
            ..inputsettings::AccessibilityKeysConfig::default()
        });
        // Everything drawn and shown first, so that the key is the only thing
        // the loop has to wake for.
        let mut server = Server::bind("127.0.0.1:0").expect("bind");
        assert!(server.compose(&mut comp));
        server.show(&comp, &mut Recording::new());
        assert_eq!(comp.wake_at(), None, "nothing is waiting yet");
        assert_eq!(server.next_wake(&comp, None, FRAME, Instant::now()), None);

        let before = Instant::now();
        // 0x1E is A. Pressing it starts the threshold rather than typing.
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1E,
            character: None,
        });
        let after = Instant::now();
        assert!(
            comp.has_deferred_key(),
            "the test premise: a key is waiting"
        );
        let due = comp.wake_at().expect("the threshold is a deadline");
        // The press happened somewhere between the two readings, and its
        // clock counts whole milliseconds, so the deadline is 300 ms after a
        // moment in that window, give or take the millisecond the clock drops.
        // Bounded by the readings rather than by a tolerance, so a machine
        // that descheduled this thread mid-press cannot fail it.
        assert!(
            due + Duration::from_millis(1) >= before + Duration::from_millis(300),
            "due {:?} after the press began, before the 300 ms threshold",
            due.saturating_duration_since(before)
        );
        assert!(
            due <= after + Duration::from_millis(300),
            "due {:?} after the press ended, after the 300 ms threshold",
            due.saturating_duration_since(after)
        );
        assert_eq!(
            server.next_wake(&comp, None, FRAME, Instant::now()),
            Some(due),
            "the loop would not wake for the key"
        );
    }

    /// Released before its threshold, the key is dropped — and so is the
    /// deadline, or the loop would wake for a keystroke that no longer exists.
    #[test]
    fn a_slow_key_released_early_leaves_no_deadline_behind() {
        let mut comp = Compositor::new(320, 240, 60).unwrap();
        comp.create_window("Editor".to_string(), 200, 150, 1);
        comp.set_accessibility_keys(inputsettings::AccessibilityKeysConfig {
            filter: inputsettings::FilterKeysConfig {
                enabled: true,
                slow_keys_ms: 300,
                ..inputsettings::FilterKeysConfig::default()
            },
            ..inputsettings::AccessibilityKeysConfig::default()
        });
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1E,
            character: None,
        });
        assert!(comp.wake_at().is_some());
        comp.handle_input(InputEvent::KeyUp { scancode: 0x1E });
        assert!(!comp.has_deferred_key());
        assert_eq!(comp.wake_at(), None);
    }

    /// An idle watch is a deadline the loop wakes for: the moment the session
    /// has been quiet for as long as the watcher asked.
    #[test]
    fn an_idle_watch_is_a_deadline_until_it_fires() {
        let mut comp = Compositor::new(320, 240, 60).unwrap();
        let window = comp.create_window("Locker".to_string(), 200, 150, 1);
        comp.watch_idle(window, Duration::from_mins(5)).unwrap();
        let due = comp.last_input() + Duration::from_mins(5);
        assert_eq!(comp.wake_at(), Some(due));

        // Two watchers: the sooner one is the deadline.
        let other = comp.create_window("Dimmer".to_string(), 200, 150, 1);
        comp.watch_idle(other, Duration::from_mins(2)).unwrap();
        assert_eq!(
            comp.wake_at(),
            Some(comp.last_input() + Duration::from_mins(2))
        );

        // Once both have fired there is nothing left to wake for...
        comp.queue_idle_notifications(due);
        assert_eq!(comp.wake_at(), None, "a fired watch is not a deadline");

        // ...until input arms them again, measured from the new input.
        comp.handle_input(InputEvent::MouseMove { x: 1, y: 1 });
        assert_eq!(
            comp.wake_at(),
            Some(comp.last_input() + Duration::from_mins(2))
        );
    }

    /// The narrowing a wait gives a tick lasts for that tick only: a caller
    /// that goes on to tick without waiting reads everyone again.
    #[test]
    fn a_wait_narrows_one_tick_and_no_more() {
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);
        // Nothing sent yet, so a short wait finds nothing.
        server.wait_for_work(
            &mut Headless,
            Some(Instant::now() + Duration::from_millis(20)),
            FRAME,
        );
        assert!(
            !server.clients[0].readable,
            "the wait found the client quiet"
        );
        assert_eq!(
            server.listener_ready, !LISTENER_READINESS,
            "and nobody connecting -- which a platform that cannot say so must not believe"
        );

        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new("Late", 100, 100)))
            .expect("send");
        // The tick the wait narrowed does not read it...
        server.tick(&mut compositor).expect("tick");
        assert!(server.clients[0].readable, "the narrowing was put back");
        // ...and the next one, not narrowed, does.
        let reply = await_reply(&mut server, &mut compositor, &mut conn, seq);
        assert!(matches!(reply, ResponseBody::WindowCreated { .. }));
    }

    /// Where the platform reports a connection waiting on the listener, the
    /// listener is asked only when it does; where it cannot (SlateOS until
    /// lane A's fix), it is asked every tick, and the loop never waits longer
    /// than a frame, so a program starting up is still accepted within one.
    #[test]
    fn a_listener_the_platform_cannot_vouch_for_is_asked_every_frame() {
        assert!(listener_worth_asking(true, true));
        assert!(
            !listener_worth_asking(true, false),
            "a reported silence is believed"
        );
        assert!(
            listener_worth_asking(false, false),
            "an unreported one is not"
        );
        assert!(listener_worth_asking(false, true));

        let now = Instant::now();
        assert_eq!(
            accept_deadline(true, now, FRAME),
            None,
            "the connection wakes the loop"
        );
        assert_eq!(accept_deadline(false, now, FRAME), Some(now + FRAME));
    }

    /// A connection arriving while the loop waits wakes it, and the client is
    /// accepted by the tick after — the listener half of waiting for work.
    #[test]
    fn a_connection_wakes_the_wait_and_is_accepted() {
        let (mut server, mut compositor, addr) = server();
        let _socket = Socket::connect(addr).expect("connect");
        let began = Instant::now();
        server.wait_for_work(
            &mut Headless,
            Some(Instant::now() + Duration::from_mins(1)),
            FRAME,
        );
        // Long before the minute the wait was allowed, not within some short
        // time a loaded machine might not meet.
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the connection did not wake the wait"
        );
        assert!(server.listener_ready);
        server.tick(&mut compositor).expect("tick");
        assert_eq!(server.client_count(), 1);
    }

    /// A client with something to say is reported by the wait and read by the
    /// tick after it.
    #[test]
    fn a_client_the_wait_found_is_read_by_the_next_tick() {
        let (mut server, mut compositor, addr) = server();
        let mut conn = dial(&mut server, &mut compositor, addr);
        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Found", 100, 100,
            )))
            .expect("send");
        server.wait_for_work(
            &mut Headless,
            Some(Instant::now() + Duration::from_mins(1)),
            FRAME,
        );
        assert!(
            server.clients[0].readable,
            "the request did not wake the wait"
        );
        server.tick(&mut compositor).expect("tick");
        for _ in 0..1000 {
            conn.pump().expect("pump");
            if let Some(reply) = conn.take_reply(seq) {
                assert!(matches!(reply, ResponseBody::WindowCreated { .. }));
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the tick after the wait did not serve the request");
    }

    // ------------------------------------------------------------------
    // Colour filter
    //
    // The filter is applied where a frame is handed to the display, so these
    // check the frame the display actually received -- not the compositor's
    // buffer, which is deliberately left unfiltered.
    // ------------------------------------------------------------------

    /// Compose one frame and return what the display was given.
    fn shown_frame(filter: ColorFilter) -> Vec<u32> {
        let (mut server, mut compositor, _addr) = server();
        compositor.set_appearance(AppearanceSettings {
            color_filter: filter,
            ..AppearanceSettings::default()
        });

        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);

        let (_, _, pixels) = display.last_frame().expect("a frame must be shown");
        pixels.to_vec()
    }

    #[test]
    fn without_a_filter_the_display_gets_the_frame_untouched() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);

        let (_, _, shown) = display.last_frame().expect("a frame");
        assert_eq!(
            shown,
            compositor.present_pixels(),
            "with no filter the display must receive the composed frame itself"
        );
    }

    /// Compose one frame with night light on, and return what the display got.
    fn warmed_frame(strength: f32) -> Vec<u32> {
        let (mut server, mut compositor, _addr) = server();
        compositor.set_appearance(AppearanceSettings {
            night_light: true,
            night_light_strength: strength,
            ..AppearanceSettings::default()
        });

        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);

        let (_, _, pixels) = display.last_frame().expect("a frame must be shown");
        pixels.to_vec()
    }

    /// Night light reaches the display, and in the direction it claims.
    ///
    /// Asserted channel by channel rather than as "the frame changed": a tint
    /// that reduced red, or that brightened blue, would also change the frame
    /// and would be the opposite of warming it.
    #[test]
    fn night_light_takes_blue_out_of_the_frame_and_leaves_red() {
        let plain = shown_frame(ColorFilter::None);
        let warm = warmed_frame(1.0);
        assert_eq!(plain.len(), warm.len(), "same frame, same size");
        assert_ne!(plain, warm, "night light must change the frame");

        for (before, after) in plain.iter().zip(warm.iter()) {
            let (r0, g0, b0) = ((before >> 16) & 0xFF, (before >> 8) & 0xFF, before & 0xFF);
            let (r1, g1, b1) = ((after >> 16) & 0xFF, (after >> 8) & 0xFF, after & 0xFF);
            assert_eq!(r1, r0, "red was changed");
            assert!(g1 <= g0, "green went up: {g0} -> {g1}");
            assert!(b1 <= b0, "blue went up: {b0} -> {b1}");
            if b0 > 0 {
                assert!(
                    b1 <= g1 || g0 == 0,
                    "blue was not cut at least as hard as green"
                );
            }
        }
    }

    /// Off is off: the frame is handed over untouched and nothing is copied.
    #[test]
    fn night_light_switched_off_changes_no_pixel() {
        let (mut server, mut compositor, _addr) = server();
        compositor.set_appearance(AppearanceSettings {
            night_light: false,
            // A strength that would be very visible if it were read.
            night_light_strength: 1.0,
            ..AppearanceSettings::default()
        });

        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);

        let (_, _, shown) = display.last_frame().expect("a frame");
        assert_eq!(
            shown,
            compositor.present_pixels(),
            "night light switched off must not touch the frame"
        );
    }

    /// A stronger setting is a warmer screen.
    ///
    /// The strength is a slider, so "it applies" is not enough: a version that
    /// ignored the number and always used full warmth would pass the test
    /// above.
    #[test]
    fn a_stronger_night_light_is_warmer() {
        let gentle = warmed_frame(0.25);
        let full = warmed_frame(1.0);

        let blue = |f: &[u32]| -> u32 { f.iter().map(|p| p & 0xFF).sum() };
        assert!(
            blue(&full) < blue(&gentle),
            "full warmth left as much blue as a quarter of it"
        );
    }

    /// The point of the whole feature: a filter changes what reaches the
    /// screen.
    #[test]
    fn a_filter_changes_the_pixels_the_display_receives() {
        let plain = shown_frame(ColorFilter::None);
        let inverted = shown_frame(ColorFilter::Inverted);

        assert_eq!(plain.len(), inverted.len(), "same frame, same size");
        assert_ne!(
            plain, inverted,
            "an inverting filter must change the frame it is applied to"
        );
    }

    /// And it is the filter's own arithmetic, not something approximate.
    #[test]
    fn the_shown_frame_is_exactly_the_filter_applied_to_the_composed_one() {
        for filter in ColorFilter::ALL {
            let (mut server, mut compositor, _addr) = server();
            compositor.set_appearance(AppearanceSettings {
                color_filter: filter,
                ..AppearanceSettings::default()
            });

            let mut display = Recording::new();
            server.compose(&mut compositor);
            let expected: Vec<u32> = compositor
                .present_pixels()
                .iter()
                .map(|p| filter.apply_argb(*p))
                .collect();
            server.show(&compositor, &mut display);

            let (_, _, shown) = display.last_frame().expect("a frame");
            assert_eq!(shown, expected.as_slice(), "{}", filter.label());
        }
    }

    /// The compositor's own buffer is left alone.
    ///
    /// It must be: the filter is applied on the way out, and composition only
    /// redraws damaged rectangles. Filtering in place would make the next
    /// frame filter an already-filtered background a second time.
    #[test]
    fn filtering_does_not_touch_the_composed_frame() {
        let (mut server, mut compositor, _addr) = server();
        compositor.set_appearance(AppearanceSettings {
            color_filter: ColorFilter::Inverted,
            ..AppearanceSettings::default()
        });

        server.compose(&mut compositor);
        let before = compositor.present_pixels().to_vec();

        let mut display = Recording::new();
        server.show(&compositor, &mut display);
        server.show(&compositor, &mut display);

        assert_eq!(
            compositor.present_pixels(),
            before.as_slice(),
            "showing a frame twice must not filter it twice"
        );
    }

    #[test]
    fn no_filter_means_no_buffer_is_allocated() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);

        assert!(
            server.filtered.is_empty(),
            "a user with no filter must not pay for one"
        );
    }

    // ------------------------------------------------------------------
    // The pointer
    // ------------------------------------------------------------------

    /// A pointer moving over a still desktop is shown without anything being
    /// composed: the same picture, under a pointer that moved.
    #[test]
    fn a_pointer_moving_over_a_still_desktop_is_shown_without_composing() {
        let (mut server, mut compositor, _addr) = server();
        let mut display = Recording::new();
        assert!(server.compose(&mut compositor));
        server.show(&compositor, &mut display);
        let serial = display.last_serial();
        assert!(serial.is_some(), "the server does not stamp its frames");
        assert!(!server.pointer_changed(&compositor), "nothing moved yet");

        compositor.handle_input(InputEvent::MouseMove { x: 100, y: 120 });
        assert!(
            !server.compose(&mut compositor),
            "a pointer move is not damage"
        );
        assert!(
            server.pointer_changed(&compositor),
            "and yet a frame is owed"
        );
        server.show(&compositor, &mut display);

        assert_eq!(display.last_serial(), serial, "the picture did not change");
        let pointer = display.last_pointer().expect("a pointer");
        let hot = (
            pointer.x + i32::try_from(pointer.image.hot_x).unwrap(),
            pointer.y + i32::try_from(pointer.image.hot_y).unwrap(),
        );
        assert_eq!(
            hot,
            (100, 120),
            "the pointer's hot spot is not where the mouse is"
        );
        assert!(
            !server.pointer_changed(&compositor),
            "the frame owed was paid"
        );
    }

    /// Night light warms the pointer too: a white arrow on a warmed screen
    /// would be the one cold thing on it.
    #[test]
    fn a_filter_reaches_the_pointer_as_well_as_the_picture() {
        let (mut server, mut compositor, _addr) = server();
        compositor.set_appearance(AppearanceSettings {
            night_light: true,
            night_light_strength: 1.0,
            ..AppearanceSettings::default()
        });
        let mut display = Recording::new();
        server.compose(&mut compositor);
        server.show(&compositor, &mut display);
        let pointer = display.last_pointer().expect("a pointer");
        // Somewhere in the white body of the arrow, just below its tip.
        let (hx, hy) = (pointer.image.hot_x, pointer.image.hot_y);
        let px = pointer
            .image
            .pixel(hx + 2, hy + 8)
            .expect("inside the image");
        let (r, b) = ((px >> 16) & 0xFF, px & 0xFF);
        assert!(r > 0 && b < r, "the pointer was not warmed: {px:#010x}");
    }
}
