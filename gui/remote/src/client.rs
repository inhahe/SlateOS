//! The client half of the display protocol: one event loop, written once.
//!
//! There are 142 application crates in this tree and, before this module, not
//! one of them was connected to anything. Each was a model plus a renderer with
//! no driver — see `known-issues.md` →
//! `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`. The obvious repair is to give each
//! app an event loop, and that is the wrong repair: 142 hand-written loops are
//! 142 chances to get the seam subtly different, and the differences would be
//! exactly the sort nobody notices until two apps behave unlike each other for
//! no reason a user can name. So the loop lives here, once, and an application
//! supplies only the part that is actually its own: what to do with an event,
//! and what to draw.
//!
//! ## Why here rather than in `guitk`
//!
//! `guiremote` depends on `guitk`; the reverse would be a cycle. The loop needs
//! both the event vocabulary (`guitk::event`) and the wire codec (this crate),
//! so this crate is the only place both are already in scope. It is also the
//! honest home: a loop whose job is "decode frames, dispatch, encode frames" is
//! protocol-shaped.
//!
//! ## Shape
//!
//! ```text
//!   compositor ──INPT frame──▶ Transport::read ──▶ decode ──▶ App::on_event
//!                                                                   │
//!                                                              Response
//!                                                                   │
//!   compositor ◀──ORDR frame── Transport::write ◀── encode ◀── App::render
//! ```
//!
//! [`Client::tick`] drains everything currently readable, dispatches each
//! event, and redraws **at most once** afterwards. That coalescing is the point:
//! a mouse drag arrives as a burst of moves, and an app that emitted a frame per
//! move would spend a desktop's entire frame budget redrawing one window.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque};

use guitk::event::Event;
use guitk::render::RenderTree;

use crate::DecodeError;
use crate::control::{Request, RequestBody, ResponseBody, encode_requests_into};
use crate::frame::{Frame, try_decode_any};
use crate::input::InputEvent;
use crate::submit::encode_submit_into;
use crate::window_list::WindowInfo;

/// What the loop should do after handing an event to an application.
///
/// Deliberately *not* [`guitk::event::EventResult`], whose `Consumed`/`Ignored`
/// answers a different question — whether an event should keep propagating up a
/// widget tree. An event can be consumed without changing anything visible (a
/// click on an already-selected item) and can change something visible without
/// being consumed. Reusing that enum here would be a pun that reads fine and
/// redraws at the wrong times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// Nothing the user can see changed. No frame is sent.
    Idle,
    /// Something changed; redraw before waiting again.
    Redraw,
    /// Close down: the loop finishes after this tick.
    Exit,
}

/// The part of an application the toolkit cannot write for it.
pub trait App {
    /// React to one event.
    ///
    /// Return [`Response::Redraw`] only when something visible actually
    /// changed. Returning it unconditionally is the easy mistake and costs a
    /// full frame per mouse move.
    fn on_event(&mut self, event: &Event) -> Response;

    /// Draw the current state at the current window size.
    ///
    /// Called at most once per tick, after every pending event has been seen,
    /// so it always draws the *settled* state rather than an intermediate one.
    fn render(&mut self, width: f32, height: f32) -> RenderTree;
}

/// A duplex byte pipe to the compositor.
///
/// Bytes rather than frames: framing is the loop's business, because only the
/// loop can hold the partial-frame remainder between reads. A transport that
/// tried to return whole frames would have to duplicate that buffer, and every
/// implementation would have to get it right separately.
pub trait Transport {
    /// Whatever this transport fails with.
    type Error;

    /// Append any immediately-available bytes to `buf`, returning how many.
    ///
    /// Zero is not an error — it is the ordinary state of an idle desktop.
    /// Implementations must not block; [`Self::wait`] is the blocking point.
    fn read(&mut self, buf: &mut Vec<u8>) -> Result<usize, Self::Error>;

    /// Send an encoded frame.
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Whether the connection is still usable. Once this returns `false`,
    /// [`Client::run`] stops.
    fn is_open(&self) -> bool {
        true
    }

    /// Block until there is plausibly something to read.
    ///
    /// The default does nothing, which turns [`Client::run`] into a spin loop —
    /// correct, and unacceptable in production. Every real transport overrides
    /// it. It is defaulted rather than required so that a test transport, whose
    /// data is all present up front, need not implement a wait that would
    /// never be entered.
    fn wait(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// What can go wrong driving a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError<E> {
    /// The transport failed.
    Transport(E),
    /// The compositor sent something this client could not decode. Fatal by
    /// nature: a stream is a sequence, so a frame that will not decode leaves
    /// no way to find where the next one starts.
    Protocol(DecodeError),
    /// The connection closed while a request was still waiting for its answer.
    ///
    /// Distinct from a transport error: nothing failed, the other end simply
    /// went away. A caller that treats the two alike will report a crash where
    /// the compositor merely shut down.
    Closed,
    /// The compositor refused a request and said why.
    Refused(String),
    /// The compositor answered a request with a reply of the wrong kind — a
    /// `WindowCreated` for a `SetTitle`, say. A protocol bug on one side or the
    /// other, and not something a client can sensibly recover from by guessing.
    Mismatched,
}

impl<E: core::fmt::Display> core::fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
            Self::Closed => write!(f, "connection closed before the request was answered"),
            Self::Refused(why) => write!(f, "compositor refused the request: {why}"),
            Self::Mismatched => write!(f, "compositor answered with the wrong kind of reply"),
        }
    }
}

impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for ClientError<E> {}

/// A live connection to the compositor, carrying every frame kind at once.
///
/// This is the layer that makes one socket behave like the several logical
/// channels an application thinks it has: input arrives for whichever windows
/// the client owns, and control replies arrive interleaved with it, out of any
/// useful order. `Connection` demultiplexes both, queues events in arrival
/// order, and files replies under the correlation id of the request that asked
/// for them.
///
/// [`Client`] is the single-window convenience built on top; `oswindow` is the
/// multi-window one. Neither reimplements the framing, and an application
/// should touch neither this type nor the wire — see the module docs.
pub struct Connection<T: Transport> {
    transport: T,
    /// Bytes read but not yet a whole frame. A stream does not respect frame
    /// boundaries, so this is where a frame split across two reads waits.
    inbox: Vec<u8>,
    /// Reused across sends so a steady-state redraw allocates nothing.
    outbox: Vec<u8>,
    /// The id the next request will carry.
    next_seq: u32,
    /// Input events for every window this connection owns, in arrival order.
    ///
    /// One queue rather than one per window, because order *between* windows is
    /// real: a click that focuses window B and then types into it must not be
    /// reordered against B's own events by a per-window fan-out.
    events: VecDeque<InputEvent>,
    /// Replies whose requester has not collected them yet.
    replies: BTreeMap<u32, ResponseBody>,
    /// Replies to a correlation id nobody is waiting for.
    ///
    /// Counted rather than dropped silently: a nonzero value means the
    /// compositor answered something twice, or answered a request this client
    /// never sent, and neither is discoverable if the reply just vanishes.
    unsolicited: u64,
    /// Frames a client has no business receiving — a bare `ORDR`, a `SURF`, a
    /// `CREQ`. Counted for the same reason. Not fatal: the frame decoded, so
    /// the stream is still in sync and the next frame is still findable.
    misdirected: u64,
    /// The most recent desktop window list, for a client that subscribed.
    ///
    /// The *latest* rather than a queue, because each `WLST` frame is a
    /// complete snapshot and an older one is not merely stale but wrong. A
    /// shell that fell behind should draw what the desktop looks like now, not
    /// replay what it looked like three changes ago — which is the same
    /// reasoning by which the compositor coalesces damage rather than queueing
    /// it.
    ///
    /// `None` until the first frame arrives, which is distinct from `Some([])`:
    /// "not told yet" and "told, and the desktop is empty" are different, and a
    /// shell that conflated them would blank its taskbar during startup.
    window_list: Option<Vec<WindowInfo>>,
    /// Increments on every window-list frame received.
    ///
    /// Lets a shell repaint on change without diffing the list against its own
    /// copy — and, unlike a dirty flag, cannot be lost by two consumers, since
    /// each remembers the number it last acted on.
    window_list_revision: u64,
}

impl<T: Transport> Connection<T> {
    /// Wrap a transport. No traffic happens until something asks for it.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            inbox: Vec::new(),
            outbox: Vec::new(),
            // Starts at 1 so that 0 can mean "no request" to a caller that
            // wants a sentinel, and never reaches 0 again on wrap.
            next_seq: 1,
            events: VecDeque::new(),
            replies: BTreeMap::new(),
            unsolicited: 0,
            misdirected: 0,
            window_list: None,
            window_list_revision: 0,
        }
    }

    /// The underlying transport.
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// The underlying transport, mutably.
    pub const fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Whether the connection is still usable.
    pub fn is_open(&self) -> bool {
        self.transport.is_open()
    }

    /// Block until there is plausibly something to read.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the transport's wait fails.
    pub fn wait(&mut self) -> Result<(), ClientError<T::Error>> {
        self.transport.wait().map_err(ClientError::Transport)
    }

    /// How many replies arrived for a correlation id nobody was waiting on.
    #[must_use]
    pub const fn unsolicited_replies(&self) -> u64 {
        self.unsolicited
    }

    /// How many frames arrived that a client should never be sent.
    #[must_use]
    pub const fn misdirected_frames(&self) -> u64 {
        self.misdirected
    }

    /// How many input events are queued and undelivered.
    #[must_use]
    pub fn pending_events(&self) -> usize {
        self.events.len()
    }

    /// Read whatever is immediately available and sort every complete frame in
    /// it into the event queue or the reply table.
    ///
    /// Returns how many frames were decoded. Never blocks: an idle desktop
    /// returns `Ok(0)`.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the read fails, [`ClientError::Protocol`]
    /// if a frame will not decode — fatal, because a stream is a sequence and a
    /// frame that will not decode hides where the next one starts.
    pub fn pump(&mut self) -> Result<usize, ClientError<T::Error>> {
        self.transport
            .read(&mut self.inbox)
            .map_err(ClientError::Transport)?;

        let mut frames = 0usize;
        let mut consumed = 0usize;
        loop {
            let rest = self.inbox.get(consumed..).unwrap_or(&[]);
            let Some((frame, used)) = try_decode_any(rest).map_err(ClientError::Protocol)? else {
                // A partial frame: leave it for the next read.
                break;
            };
            // A zero-length frame would spin here forever. It cannot happen —
            // every frame is at least a header — but this loop should not
            // depend on that being true of a *remote* encoder we do not control.
            if used == 0 {
                break;
            }
            consumed = consumed.saturating_add(used);
            frames = frames.saturating_add(1);
            self.file(frame);
        }
        // Drained from the front rather than reallocated, so a steady stream
        // reuses one buffer instead of copying the remainder every pump.
        self.inbox.drain(..consumed);
        Ok(frames)
    }

    /// Put one decoded frame where its consumer will look for it.
    fn file(&mut self, frame: Frame) {
        match frame {
            Frame::Input(events) => self.events.extend(events),
            Frame::Responses(responses) => {
                for r in responses {
                    // A second answer to one request keeps the *first*. The
                    // first is what a requester already blocked on `round_trip`
                    // will have been handed, so letting a later one overwrite
                    // it would mean two callers of the same request seeing
                    // different answers depending only on their timing.
                    match self.replies.entry(r.seq) {
                        Entry::Vacant(slot) => {
                            slot.insert(r.body);
                        }
                        Entry::Occupied(_) => {
                            self.unsolicited = self.unsolicited.saturating_add(1);
                        }
                    }
                }
            }
            // Kept even if this client never subscribed. A frame that arrived
            // is a frame the compositor decided to send, and dropping it as
            // "unrequested" would make an unsubscribe race look like a
            // protocol error rather than the ordinary crossing it is.
            Frame::WindowList(windows) => {
                self.window_list = Some(windows);
                self.window_list_revision = self.window_list_revision.saturating_add(1);
            }
            // Everything else travels the other way. A compositor that sends
            // one is misrouting; that is worth being able to see and is not
            // worth killing an application over.
            Frame::Render(_) | Frame::Submit(_) | Frame::Scene(_) | Frame::Requests(_) => {
                self.misdirected = self.misdirected.saturating_add(1);
            }
        }
    }

    /// The desktop's windows as of the last `WLST` frame, or `None` if none has
    /// arrived — which is every client that never subscribed, and a subscribed
    /// one that has not pumped since.
    ///
    /// Bottom-to-top stacking order. See
    /// [`window_list`](crate::window_list) for what a shell does with it.
    #[must_use]
    pub fn window_list(&self) -> Option<&[WindowInfo]> {
        self.window_list.as_deref()
    }

    /// How many window lists have arrived. A shell repaints when this moves.
    #[must_use]
    pub const fn window_list_revision(&self) -> u64 {
        self.window_list_revision
    }

    /// Start or stop receiving the desktop window list, waiting for the
    /// compositor to acknowledge.
    ///
    /// The list itself arrives later, on the next pump; this only confirms the
    /// subscription was accepted.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the exchange fails, and
    /// [`ClientError::Refused`] if the compositor declined.
    pub fn subscribe_window_list(&mut self, on: bool) -> Result<(), ClientError<T::Error>> {
        self.confirm(RequestBody::SubscribeWindowList { subscribe: on })
    }

    /// Take the oldest queued input event, if any.
    pub fn next_event(&mut self) -> Option<InputEvent> {
        self.events.pop_front()
    }

    /// Take every queued input event.
    pub fn drain_events(&mut self) -> Vec<InputEvent> {
        self.events.drain(..).collect()
    }

    /// Collect the reply to `seq` if it has arrived.
    pub fn take_reply(&mut self, seq: u32) -> Option<ResponseBody> {
        self.replies.remove(&seq)
    }

    /// Send a request and return the correlation id its reply will carry.
    ///
    /// Does not wait. Use this when several requests should go out before any
    /// of them is answered — mapping three windows at startup costs one round
    /// trip this way and three if each is awaited in turn.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the write fails.
    pub fn send(&mut self, body: RequestBody) -> Result<u32, ClientError<T::Error>> {
        let seq = self.next_seq;
        // Wrapping is correct rather than saturating: an id is only ever
        // compared for equality, and skipping 0 keeps it available as a
        // sentinel. Four billion outstanding requests is not a situation.
        self.next_seq = match self.next_seq.wrapping_add(1) {
            0 => 1,
            n => n,
        };
        self.outbox.clear();
        encode_requests_into(&mut self.outbox, &[Request { seq, body }]);
        // The borrow checker will not let the transport borrow `self.outbox`
        // while `self` is borrowed mutably, and a temporary swap costs a
        // pointer move rather than a copy of the buffer.
        let buf = core::mem::take(&mut self.outbox);
        let result = self.transport.write(&buf);
        self.outbox = buf;
        result.map_err(ClientError::Transport)?;
        Ok(seq)
    }

    /// Send a request and block until its answer arrives.
    ///
    /// # Errors
    ///
    /// As [`Self::send`], plus [`ClientError::Closed`] if the connection ends
    /// before the answer arrives.
    pub fn round_trip(&mut self, body: RequestBody) -> Result<ResponseBody, ClientError<T::Error>> {
        let seq = self.send(body)?;
        loop {
            // Pumped before the open check so that a reply already sitting in
            // the buffer of a transport that has since closed is still
            // delivered — the answer arrived, and the shutdown after it does
            // not un-arrive it.
            self.pump()?;
            if let Some(reply) = self.take_reply(seq) {
                return Ok(reply);
            }
            if !self.is_open() {
                return Err(ClientError::Closed);
            }
            self.wait()?;
        }
    }

    /// Ask the compositor for a window and block until it exists.
    ///
    /// # Errors
    ///
    /// As [`Self::round_trip`], plus [`ClientError::Refused`] if the compositor
    /// declined and [`ClientError::Mismatched`] if it answered with something
    /// other than a window.
    pub fn create_window(
        &mut self,
        spec: crate::control::WindowSpec,
    ) -> Result<u64, ClientError<T::Error>> {
        match self.round_trip(RequestBody::CreateWindow(spec))? {
            ResponseBody::WindowCreated { window } => Ok(window),
            ResponseBody::Error { message } => Err(ClientError::Refused(message)),
            ResponseBody::Ok | ResponseBody::Display(_) => Err(ClientError::Mismatched),
        }
    }

    /// Send a request that expects no answer beyond acknowledgement, and wait
    /// for that acknowledgement.
    ///
    /// Waiting matters even though the reply carries nothing: an unacknowledged
    /// `SetTitle` that the compositor refused would otherwise fail silently.
    ///
    /// # Errors
    ///
    /// As [`Self::round_trip`], plus [`ClientError::Refused`] /
    /// [`ClientError::Mismatched`].
    pub fn confirm(&mut self, body: RequestBody) -> Result<(), ClientError<T::Error>> {
        match self.round_trip(body)? {
            ResponseBody::Ok => Ok(()),
            ResponseBody::Error { message } => Err(ClientError::Refused(message)),
            ResponseBody::WindowCreated { .. } | ResponseBody::Display(_) => {
                Err(ClientError::Mismatched)
            }
        }
    }

    /// Send one window's picture.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the write fails.
    pub fn submit(&mut self, window: u64, tree: &RenderTree) -> Result<(), ClientError<T::Error>> {
        self.outbox.clear();
        encode_submit_into(window, tree, &mut self.outbox);
        let buf = core::mem::take(&mut self.outbox);
        let result = self.transport.write(&buf);
        self.outbox = buf;
        result.map_err(ClientError::Transport)
    }
}

/// Drives one window: reads events, dispatches them, sends frames.
pub struct Client<T: Transport> {
    conn: Connection<T>,
    window: u64,
    width: u32,
    height: u32,
    /// Set until the first frame has been sent. A window that has never been
    /// drawn is blank, and nothing else would ever ask for that first paint —
    /// the app has had no event to respond to yet.
    needs_first_paint: bool,
    focused: bool,
    exiting: bool,
    /// Events addressed to some other window.
    ///
    /// Counted rather than silently dropped. A nonzero value here means either
    /// this client owns windows it did not tell the loop about, or the
    /// compositor is mis-routing — both worth being able to see, and neither
    /// discoverable if the events simply vanish.
    stray_events: u64,
}

impl<T: Transport> Client<T> {
    /// Bind a transport to a window that already exists.
    pub fn new(transport: T, window: u64, width: u32, height: u32) -> Self {
        Self::over(Connection::new(transport), window, width, height)
    }

    /// Bind an existing connection to a window that already exists.
    ///
    /// Useful when the connection has been used for something else first —
    /// asking for the display size before choosing how big to be, say.
    pub const fn over(conn: Connection<T>, window: u64, width: u32, height: u32) -> Self {
        Self {
            conn,
            window,
            width,
            height,
            needs_first_paint: true,
            focused: false,
            exiting: false,
            stray_events: 0,
        }
    }

    /// Ask the compositor for a window, then drive it.
    ///
    /// The size comes from the spec rather than being passed separately,
    /// because the two must agree: a client that asked for 800×600 and then
    /// told itself it was 1024×768 would draw its first frame at a size the
    /// window does not have.
    ///
    /// # Errors
    ///
    /// As [`Connection::create_window`].
    pub fn open(
        transport: T,
        spec: crate::control::WindowSpec,
    ) -> Result<Self, ClientError<T::Error>> {
        let (width, height) = (spec.width, spec.height);
        let mut conn = Connection::new(transport);
        let window = conn.create_window(spec)?;
        Ok(Self::over(conn, window, width, height))
    }

    /// The connection underneath, for requests this loop does not wrap.
    pub const fn connection(&mut self) -> &mut Connection<T> {
        &mut self.conn
    }

    /// The underlying transport.
    pub const fn transport(&self) -> &T {
        self.conn.transport()
    }

    /// The window this client is driving.
    #[must_use]
    pub const fn window(&self) -> u64 {
        self.window
    }

    /// The current window size, kept up to date from `Resize` events.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Whether this window currently has keyboard focus.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// How many events arrived addressed to a different window.
    #[must_use]
    pub const fn stray_events(&self) -> u64 {
        self.stray_events
    }

    /// Whether the loop has been asked to stop.
    #[must_use]
    pub const fn is_exiting(&self) -> bool {
        self.exiting
    }

    /// Run one iteration: drain readable input, dispatch it, redraw if needed.
    ///
    /// Returns `false` once the application is finished.
    pub fn tick(&mut self, app: &mut dyn App) -> Result<bool, ClientError<T::Error>> {
        let events = self.receive()?;

        let mut redraw = self.needs_first_paint;
        for ev in events {
            if ev.window != self.window {
                self.stray_events = self.stray_events.saturating_add(1);
                continue;
            }
            // Applied before the app sees the event, so an app handling
            // `Resize` can already read the new size from the client, and so
            // that the render below uses the new size rather than the previous
            // one — a frame drawn at the old size is a visibly stretched or
            // clipped window for one refresh.
            self.apply(&ev.event);

            match app.on_event(&ev.event) {
                Response::Idle => {}
                Response::Redraw => redraw = true,
                Response::Exit => {
                    self.exiting = true;
                    // Keep dispatching the rest of the batch: they arrived
                    // before the decision to exit and an app may need them to
                    // save its state. The loop ends after this tick either way.
                }
            }
            // A close request the app did not act on still closes the window.
            // The alternative is a window with a close button that does
            // nothing, which is worse than closing an app that wanted to stay.
            if matches!(ev.event, Event::CloseRequested) {
                self.exiting = true;
            }
        }

        if redraw && !self.exiting {
            self.draw(app)?;
        }
        Ok(!self.exiting)
    }

    /// Tick until the application exits or the transport closes.
    pub fn run(&mut self, app: &mut dyn App) -> Result<(), ClientError<T::Error>> {
        while self.conn.is_open() {
            if !self.tick(app)? {
                break;
            }
            self.conn.wait()?;
        }
        Ok(())
    }

    /// Draw and send one frame, whether or not anything asked for it.
    ///
    /// Public because a redraw is not always event-driven: an animation or a
    /// completed background load needs a frame with no input behind it.
    pub fn draw(&mut self, app: &mut dyn App) -> Result<(), ClientError<T::Error>> {
        #[allow(clippy::cast_precision_loss)]
        let tree = app.render(self.width as f32, self.height as f32);
        self.conn.submit(self.window, &tree)?;
        self.needs_first_paint = false;
        Ok(())
    }

    /// Read whatever is available and take the events out of it.
    fn receive(&mut self) -> Result<Vec<InputEvent>, ClientError<T::Error>> {
        self.conn.pump()?;
        Ok(self.conn.drain_events())
    }

    /// Fold an event into the client's own view of the window.
    fn apply(&mut self, event: &Event) {
        match *event {
            Event::Resize { width, height } => {
                self.width = width;
                self.height = height;
            }
            Event::FocusIn => self.focused = true,
            Event::FocusOut => self.focused = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use guitk::color::Color;
    use guitk::event::{Key, KeyEvent, Modifiers, MouseEvent, MouseEventKind};

    use super::*;
    use crate::input::encode_input_frame;

    /// A transport whose input is scripted and whose output is kept for
    /// inspection. `chunks` lets a test hand over a frame in pieces, which is
    /// how a real socket delivers one.
    #[derive(Default)]
    struct FakeTransport {
        chunks: Vec<Vec<u8>>,
        sent: Vec<Vec<u8>>,
        open: bool,
        waits: usize,
    }

    impl FakeTransport {
        fn new(chunks: Vec<Vec<u8>>) -> Self {
            Self {
                chunks,
                sent: Vec::new(),
                open: true,
                waits: 0,
            }
        }
    }

    impl Transport for FakeTransport {
        type Error = &'static str;

        fn read(&mut self, buf: &mut Vec<u8>) -> Result<usize, Self::Error> {
            if self.chunks.is_empty() {
                return Ok(0);
            }
            let chunk = self.chunks.remove(0);
            buf.extend_from_slice(&chunk);
            Ok(chunk.len())
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.sent.push(bytes.to_vec());
            Ok(())
        }

        fn is_open(&self) -> bool {
            self.open
        }

        fn wait(&mut self) -> Result<(), Self::Error> {
            self.waits += 1;
            // Nothing left to deliver means nothing will ever arrive, so the
            // loop would otherwise spin forever in a test.
            if self.chunks.is_empty() {
                self.open = false;
            }
            Ok(())
        }
    }

    /// Records what it was told and how often it was asked to draw.
    #[derive(Default)]
    struct RecordingApp {
        seen: Vec<Event>,
        renders: Vec<(f32, f32)>,
        response: Option<Response>,
    }

    impl App for RecordingApp {
        fn on_event(&mut self, event: &Event) -> Response {
            self.seen.push(event.clone());
            self.response.unwrap_or(Response::Idle)
        }

        fn render(&mut self, width: f32, height: f32) -> RenderTree {
            self.renders.push((width, height));
            let mut tree = RenderTree::new();
            tree.fill_rect(0.0, 0.0, width, height, Color::from_hex(0x11_11_11));
            tree
        }
    }

    fn key_frame(window: u64, k: Key) -> Vec<u8> {
        encode_input_frame(&[InputEvent::key(
            window,
            KeyEvent {
                key: k,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            },
            0x1E,
        )])
    }

    fn client(chunks: Vec<Vec<u8>>) -> Client<FakeTransport> {
        Client::new(FakeTransport::new(chunks), 1, 800, 600)
    }

    /// A client whose first read returns nothing, so the first tick is a bare
    /// first paint and the scripted frames land on the ticks after it.
    ///
    /// This is what connecting actually looks like: the app maps a window and
    /// paints it before the compositor has anything to say. Tests that want to
    /// observe the first paint *separately* have to ask for that read, because
    /// a tick that finds input waiting deliberately folds the two together —
    /// see `the_first_paint_folds_into_the_first_batch`.
    fn quiet_then(chunks: Vec<Vec<u8>>) -> Client<FakeTransport> {
        let mut all = vec![Vec::new()];
        all.extend(chunks);
        client(all)
    }

    #[test]
    fn the_first_paint_folds_into_the_first_batch() {
        // If input is already waiting when the app first ticks, painting before
        // reading it would put a frame on screen that the very next frame
        // corrects — and if that input is the opening `Resize`, the first frame
        // is drawn at a size the window does not have.
        let frame = encode_input_frame(&[InputEvent::new(
            1,
            Event::Resize {
                width: 1024,
                height: 768,
            },
        )]);
        let mut c = client(vec![frame]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        assert_eq!(
            app.renders,
            vec![(1024.0, 768.0)],
            "one frame, at the size the resize just established"
        );
    }

    #[test]
    fn the_first_tick_paints_even_with_no_events() {
        // A window nothing has drawn is blank, and no event will ever ask for
        // that first frame — the app has had nothing to respond to.
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        assert!(c.tick(&mut app).unwrap());
        assert_eq!(app.renders, vec![(800.0, 600.0)]);
        assert_eq!(c.transport().sent.len(), 1);
    }

    #[test]
    fn a_quiet_tick_after_the_first_paint_sends_nothing() {
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        c.tick(&mut app).unwrap();
        assert_eq!(app.renders.len(), 1, "an idle app must not redraw");
        assert_eq!(c.transport().sent.len(), 1);
    }

    #[test]
    fn an_event_that_changes_nothing_visible_does_not_redraw() {
        let mut c = quiet_then(vec![key_frame(1, Key::A)]);
        let mut app = RecordingApp {
            response: Some(Response::Idle),
            ..RecordingApp::default()
        };
        c.tick(&mut app).unwrap(); // first paint, no input consumed yet
        c.tick(&mut app).unwrap();
        assert_eq!(app.seen.len(), 1, "the event must still be delivered");
        assert_eq!(app.renders.len(), 1, "but must not have caused a frame");
    }

    #[test]
    fn a_burst_of_events_coalesces_into_one_frame() {
        // The reason the loop redraws after the batch rather than per event.
        // Ten mouse moves producing ten frames would spend a desktop's whole
        // budget on one window.
        let moves: Vec<InputEvent> = (0u8..10)
            .map(|i| {
                InputEvent::new(
                    1,
                    Event::Mouse(MouseEvent {
                        x: f32::from(i),
                        y: 0.0,
                        kind: MouseEventKind::Move,
                    }),
                )
            })
            .collect();
        let mut c = quiet_then(vec![encode_input_frame(&moves)]);
        let mut app = RecordingApp {
            response: Some(Response::Redraw),
            ..RecordingApp::default()
        };
        c.tick(&mut app).unwrap(); // first paint
        c.tick(&mut app).unwrap();
        assert_eq!(app.seen.len(), 10, "every event must be delivered");
        assert_eq!(
            app.renders.len(),
            2,
            "one first paint, one coalesced redraw"
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_not_lost() {
        // Exactly what a socket does. The half-frame has to wait in the buffer
        // rather than being decoded as garbage or dropped.
        let frame = key_frame(1, Key::Z);
        let mid = frame.len() / 2;
        let mut c = client(vec![frame[..mid].to_vec(), frame[mid..].to_vec()]);
        let mut app = RecordingApp::default();

        c.tick(&mut app).unwrap(); // reads the first half
        assert!(app.seen.is_empty(), "half a frame is not an event");
        c.tick(&mut app).unwrap(); // reads the rest
        assert_eq!(app.seen.len(), 1);
        assert!(matches!(app.seen[0], Event::Key(_)));
    }

    #[test]
    fn several_frames_in_one_read_are_all_processed() {
        let mut buf = key_frame(1, Key::A);
        buf.extend_from_slice(&key_frame(1, Key::B));
        buf.extend_from_slice(&key_frame(1, Key::C));
        let mut c = client(vec![buf]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        assert_eq!(app.seen.len(), 3);
    }

    #[test]
    fn a_resize_is_applied_before_the_frame_that_answers_it() {
        // Drawing the answering frame at the old size is a visibly stretched
        // window for one refresh.
        let frame = encode_input_frame(&[InputEvent::new(
            1,
            Event::Resize {
                width: 1024,
                height: 768,
            },
        )]);
        let mut c = quiet_then(vec![frame]);
        let mut app = RecordingApp {
            response: Some(Response::Redraw),
            ..RecordingApp::default()
        };
        c.tick(&mut app).unwrap(); // first paint at 800x600
        c.tick(&mut app).unwrap();
        assert_eq!(c.size(), (1024, 768));
        assert_eq!(app.renders, vec![(800.0, 600.0), (1024.0, 768.0)]);
    }

    #[test]
    fn focus_is_tracked_from_the_events_that_report_it() {
        let frame = encode_input_frame(&[
            InputEvent::new(1, Event::FocusIn),
            InputEvent::new(1, Event::FocusOut),
            InputEvent::new(1, Event::FocusIn),
        ]);
        let mut c = client(vec![frame]);
        let mut app = RecordingApp::default();
        assert!(!c.is_focused());
        c.tick(&mut app).unwrap();
        assert!(c.is_focused());
    }

    #[test]
    fn an_event_for_another_window_is_counted_not_acted_on() {
        // Silently dropping it would make a routing bug invisible.
        let mut c = client(vec![key_frame(99, Key::A)]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        assert!(app.seen.is_empty(), "must not be dispatched");
        assert_eq!(c.stray_events(), 1, "but must be visible to a debugger");
    }

    #[test]
    fn a_close_request_ends_the_loop_even_if_the_app_ignores_it() {
        // A close button that does nothing is worse than an app that closes
        // when it would rather not have.
        let frame = encode_input_frame(&[InputEvent::new(1, Event::CloseRequested)]);
        let mut c = client(vec![frame]);
        let mut app = RecordingApp {
            response: Some(Response::Idle),
            ..RecordingApp::default()
        };
        c.tick(&mut app).unwrap(); // first paint
        assert!(!c.tick(&mut app).unwrap(), "loop must report it is done");
        assert!(c.is_exiting());
    }

    #[test]
    fn an_app_asking_to_exit_still_sees_the_rest_of_the_batch() {
        // Those events arrived before the decision; an app may need them to
        // save its state.
        let mut buf = key_frame(1, Key::A);
        buf.extend_from_slice(&key_frame(1, Key::B));
        buf.extend_from_slice(&key_frame(1, Key::C));
        let mut c = client(vec![buf]);
        let mut app = RecordingApp {
            response: Some(Response::Exit),
            ..RecordingApp::default()
        };
        assert!(!c.tick(&mut app).unwrap());
        assert_eq!(app.seen.len(), 3);
    }

    #[test]
    fn an_exiting_tick_does_not_paint() {
        // A frame drawn for a window that is closing is work nobody sees.
        let frame = encode_input_frame(&[InputEvent::new(1, Event::CloseRequested)]);
        let mut c = client(vec![frame]);
        let mut app = RecordingApp {
            response: Some(Response::Redraw),
            ..RecordingApp::default()
        };
        c.tick(&mut app).unwrap(); // first paint
        let before = app.renders.len();
        c.tick(&mut app).unwrap();
        assert_eq!(app.renders.len(), before);
    }

    #[test]
    fn what_is_sent_is_an_addressed_decodable_draw_frame() {
        // The other half of the loop: a client's output must be exactly what a
        // compositor's decoder accepts — and must say which window it is for,
        // since the compositor cannot infer that from a connection that may
        // carry several.
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        let sent = &c.transport().sent[0];
        let (sub, used) = crate::submit::decode_submit(sent).unwrap();
        assert_eq!(used, sent.len());
        assert_eq!(sub.window, 1, "addressed to the window this client drives");
        assert_eq!(sub.commands.commands.len(), 1);
    }

    #[test]
    fn a_corrupt_input_frame_is_a_fatal_protocol_error() {
        // A stream is a sequence: a frame that will not decode leaves no way to
        // find where the next one begins, so carrying on would be guessing.
        let mut frame = key_frame(1, Key::A);
        frame[0] = b'X'; // break the magic
        let mut c = client(vec![frame]);
        let mut app = RecordingApp::default();
        assert_eq!(
            c.tick(&mut app),
            Err(ClientError::Protocol(DecodeError::BadMagic))
        );
    }

    #[test]
    fn run_stops_when_the_transport_closes() {
        let mut c = client(vec![key_frame(1, Key::A)]);
        let mut app = RecordingApp::default();
        c.run(&mut app).unwrap();
        assert!(!c.transport().is_open());
        assert_eq!(app.seen.len(), 1);
    }

    #[test]
    fn run_stops_when_the_app_exits_without_waiting_again() {
        let frame = encode_input_frame(&[InputEvent::new(1, Event::CloseRequested)]);
        let mut c = quiet_then(vec![frame]);
        let mut app = RecordingApp::default();
        c.run(&mut app).unwrap();
        assert!(c.is_exiting());
        assert_eq!(
            c.transport().waits,
            1,
            "must not wait again after deciding to exit"
        );
    }

    #[test]
    fn an_explicit_draw_needs_no_event_behind_it() {
        // Animations and completed background work redraw with no input.
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        c.draw(&mut app).unwrap();
        assert_eq!(app.renders.len(), 2);
        assert_eq!(c.transport().sent.len(), 2);
    }

    // ------------------------------------------------------------------
    // Connection — the demultiplexing layer underneath
    // ------------------------------------------------------------------

    use crate::control::{RequestBody, ResponseBody, WindowSpec, encode_responses};

    fn conn(chunks: Vec<Vec<u8>>) -> Connection<FakeTransport> {
        Connection::new(FakeTransport::new(chunks))
    }

    fn reply(seq: u32, body: ResponseBody) -> Vec<u8> {
        encode_responses(&[crate::control::Response::new(seq, body)])
    }

    #[test]
    fn input_and_replies_interleaved_on_one_connection_both_arrive() {
        // The reason this layer exists. A client has one socket; a naive reader
        // that only understood input frames would choke on the first reply, and
        // one that only understood replies would lose every keystroke.
        let mut buf = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        buf.extend_from_slice(&reply(1, ResponseBody::WindowCreated { window: 42 }));
        buf.extend_from_slice(&encode_input_frame(&[InputEvent::new(
            1,
            Event::CloseRequested,
        )]));

        let mut c = conn(vec![buf]);
        assert_eq!(c.pump().unwrap(), 3, "three frames of two kinds");
        assert_eq!(c.pending_events(), 2);
        assert_eq!(
            c.take_reply(1),
            Some(ResponseBody::WindowCreated { window: 42 })
        );
        let events = c.drain_events();
        assert!(matches!(events[0].event, Event::FocusIn));
        assert!(matches!(events[1].event, Event::CloseRequested));
    }

    #[test]
    fn events_keep_their_arrival_order_across_windows() {
        // One queue rather than one per window: a click that focuses B and then
        // types into it must not be reordered by a per-window fan-out.
        let buf = encode_input_frame(&[
            InputEvent::new(1, Event::FocusOut),
            InputEvent::new(2, Event::FocusIn),
            InputEvent::new(2, Event::Tick { elapsed_ms: 16 }),
            InputEvent::new(1, Event::Tick { elapsed_ms: 16 }),
        ]);
        let mut c = conn(vec![buf]);
        c.pump().unwrap();
        let order: Vec<u64> = c.drain_events().iter().map(|e| e.window).collect();
        assert_eq!(order, vec![1, 2, 2, 1]);
    }

    #[test]
    fn a_reply_is_filed_under_the_request_that_asked_for_it() {
        // Replies may arrive in any order; correlation, not arrival order, is
        // what pairs an answer with its question.
        let mut c = conn(vec![]);
        let first = c.send(RequestBody::GetDisplayInfo).unwrap();
        let second = c.send(RequestBody::Minimize { window: 5 }).unwrap();
        assert_ne!(first, second, "two requests must not share an id");

        // Answered back to front.
        c.transport_mut()
            .chunks
            .push(reply(second, ResponseBody::Ok));
        c.transport_mut().chunks.push(reply(
            first,
            ResponseBody::Display(crate::control::DisplayInfo {
                width: 1920,
                height: 1080,
                refresh_rate: 60,
                scale_factor: 1.0,
            }),
        ));
        c.pump().unwrap();
        c.pump().unwrap();

        assert_eq!(c.take_reply(second), Some(ResponseBody::Ok));
        assert!(matches!(
            c.take_reply(first),
            Some(ResponseBody::Display(_))
        ));
        assert_eq!(c.take_reply(first), None, "a reply is collected once");
    }

    #[test]
    fn a_request_goes_out_as_a_frame_the_compositor_can_decode() {
        let mut c = conn(vec![]);
        let seq = c
            .send(RequestBody::SetTitle {
                window: 3,
                title: "Notes".to_string(),
            })
            .unwrap();
        let sent = &c.transport().sent[0];
        let (reqs, used) = crate::control::decode_requests(sent).unwrap();
        assert_eq!(used, sent.len());
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].seq, seq);
        assert!(matches!(
            reqs[0].body,
            RequestBody::SetTitle { window: 3, .. }
        ));
    }

    #[test]
    fn a_round_trip_waits_for_its_own_answer_and_no_other() {
        let mut c = conn(vec![]);
        // The compositor answers a *different* request first; the round trip
        // must not mistake it for its own.
        c.transport_mut().chunks.push(reply(999, ResponseBody::Ok));
        c.transport_mut()
            .chunks
            .push(reply(1, ResponseBody::WindowCreated { window: 7 }));
        let got = c.round_trip(RequestBody::GetDisplayInfo).unwrap();
        assert_eq!(got, ResponseBody::WindowCreated { window: 7 });
    }

    #[test]
    fn create_window_reports_the_id_the_compositor_chose() {
        // Not one the client made up: window ids are the compositor's to mint,
        // and a client that allocated its own would be naming windows that do
        // not exist.
        let mut c = conn(vec![reply(
            1,
            ResponseBody::WindowCreated { window: 0xABC },
        )]);
        let id = c.create_window(WindowSpec::new("Test", 320, 240)).unwrap();
        assert_eq!(id, 0xABC);
    }

    #[test]
    fn a_refused_window_is_an_error_rather_than_a_window() {
        let mut c = conn(vec![reply(
            1,
            ResponseBody::Error {
                message: "out of memory".to_string(),
            },
        )]);
        assert_eq!(
            c.create_window(WindowSpec::new("Test", 320, 240)),
            Err(ClientError::Refused("out of memory".to_string()))
        );
    }

    #[test]
    fn an_answer_of_the_wrong_kind_is_rejected_rather_than_guessed() {
        let mut c = conn(vec![reply(1, ResponseBody::Ok)]);
        assert_eq!(
            c.create_window(WindowSpec::new("Test", 320, 240)),
            Err(ClientError::Mismatched)
        );
    }

    #[test]
    fn a_connection_that_closes_mid_request_reports_closed_not_a_hang() {
        // Nothing failed — the other end went away. A client that reported a
        // transport error here would say "crash" where "shutdown" is the truth.
        let mut c = conn(vec![]);
        assert_eq!(
            c.create_window(WindowSpec::new("Test", 320, 240)),
            Err(ClientError::Closed)
        );
    }

    #[test]
    fn an_answer_already_in_the_buffer_survives_the_close_that_follows_it() {
        // The reply arrived; a shutdown afterwards does not un-arrive it.
        let mut c = conn(vec![reply(1, ResponseBody::Ok)]);
        c.transport_mut().open = false;
        assert!(c.confirm(RequestBody::Restore { window: 1 }).is_ok());
    }

    #[test]
    fn a_frame_travelling_the_wrong_way_is_counted_not_fatal() {
        // A compositor sending a client a scene frame is misrouting. The frame
        // decoded, so the stream is still in sync and the next frame is still
        // findable — killing the app over it would be an overreaction.
        let mut buf = crate::scene::encode_scene_frame(&crate::scene::SceneFrame {
            sequence: 1,
            display_width: 1,
            display_height: 1,
            windows: Vec::new(),
            removed: Vec::new(),
        });
        buf.extend_from_slice(&encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]));
        let mut c = conn(vec![buf]);
        c.pump().unwrap();
        assert_eq!(c.misdirected_frames(), 1);
        assert_eq!(c.pending_events(), 1, "the good frame after it still lands");
    }

    #[test]
    fn a_second_answer_to_one_request_is_counted_and_does_not_displace_the_first() {
        let mut buf = reply(1, ResponseBody::Ok);
        buf.extend_from_slice(&reply(
            1,
            ResponseBody::Error {
                message: "late".to_string(),
            },
        ));
        let mut c = conn(vec![buf]);
        c.pump().unwrap();
        assert_eq!(c.unsolicited_replies(), 1);
        assert_eq!(
            c.take_reply(1),
            Some(ResponseBody::Ok),
            "the first answer is the one the requester was told about"
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled_by_the_connection_too() {
        let whole = reply(1, ResponseBody::WindowCreated { window: 4 });
        let mid = whole.len() / 2;
        let mut c = conn(vec![whole[..mid].to_vec(), whole[mid..].to_vec()]);
        assert_eq!(c.pump().unwrap(), 0, "half a frame is not a frame");
        assert_eq!(c.pump().unwrap(), 1);
        assert_eq!(
            c.take_reply(1),
            Some(ResponseBody::WindowCreated { window: 4 })
        );
    }

    #[test]
    fn a_corrupt_frame_is_fatal_because_the_next_one_becomes_unfindable() {
        let mut bytes = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        bytes[0] = b'X';
        let mut c = conn(vec![bytes]);
        assert_eq!(c.pump(), Err(ClientError::Protocol(DecodeError::BadMagic)));
    }

    #[test]
    fn a_submission_names_the_window_it_is_for() {
        let mut c = conn(vec![]);
        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 4.0, 4.0, Color::from_hex(0x01_02_03));
        c.submit(77, &tree).unwrap();
        let (sub, _) = crate::submit::decode_submit(&c.transport().sent[0]).unwrap();
        assert_eq!(sub.window, 77);
    }

    #[test]
    fn correlation_ids_never_take_the_sentinel_value() {
        // 0 is reserved so a caller can use it to mean "no request outstanding".
        let mut c = conn(vec![]);
        c.next_seq = u32::MAX;
        let last = c.send(RequestBody::GetDisplayInfo).unwrap();
        let wrapped = c.send(RequestBody::GetDisplayInfo).unwrap();
        assert_eq!(last, u32::MAX);
        assert_eq!(wrapped, 1, "wraps past 0, not onto it");
    }
}
