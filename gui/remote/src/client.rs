//! The client half of the display protocol: one multiplexed connection.
//!
//! [`Connection`] owns the socket and is the only thing that reads it. Two
//! kinds of frame arrive down the same pipe — input events pushed by the
//! compositor, and replies to requests this client made — so something has to
//! demultiplex them. That is the whole job here: read bytes, reassemble frames
//! across read boundaries, file each one under *input* or *reply to request
//! `seq`*, and encode outbound requests and submissions.
//!
//! A naive client reads the socket looking for its own reply and drops the
//! input frames that arrive first. [`Connection::round_trip`] queues them
//! instead, which is why `next_event`/`drain_events` exist alongside the
//! request API.
//!
//! ## Shape
//!
//! ```text
//!   compositor ──INPT frame──▶ Transport::read ──▶ decode ──┬─▶ event queue
//!                                                           └─▶ reply slot
//!
//!   compositor ◀──ORDR/CTRL─── Transport::write ◀── encode ◀── submit/request
//! ```
//!
//! ## Where the event loop went
//!
//! This module used to carry one too — a `Client` wrapping a `Connection`, with
//! an `App` trait and a `Response` verdict. It was retired in favour of
//! `oswindow::app`, and the reason is worth keeping: `Client` had no wake-up
//! list. It parked in [`Transport::wait`] until the compositor said something,
//! so an application built on it could never receive a timer tick it had asked
//! for. A harness that cannot deliver `Event::Tick` reproduces `known-issues.md`
//! lesson 47 — *an app that keeps time but never receives the clock* — for every
//! app built on it, and the frozen stopwatch would have been the harness's fault
//! rather than the app's.
//!
//! `oswindow::EventLoop` already had the frame clock, so the loop with the clock
//! survived and the vocabulary (`App`, `Response`) moved to it unchanged. The
//! coalescing rule `Client::tick` enforced — drain everything readable, dispatch
//! each event, redraw **at most once** afterwards — moved with it as
//! `EventLoop::run_batched` and its `Dispatch::Settled`.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use guitk::render::RenderTree;

use crate::DecodeError;
use crate::control::{
    Request, RequestBody, ResponseBody, ShellControlAction, encode_requests_into,
};
use crate::frame::{Frame, try_decode_any};
use crate::input::InputEvent;
use crate::submit::encode_submit_into;
use crate::window_list::{WindowInfo, WindowList};

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

    /// Cap how long [`Self::wait`] parks. `None` means park until something
    /// arrives, which is the default and the right answer for an idle desktop.
    ///
    /// This is what lets a caller keep a deadline of its own — an animation
    /// frame, a blinking caret — without giving up the blocking wait and
    /// polling instead. `wait` must return no later than `timeout` once this
    /// has been set, though it may of course return sooner because input
    /// arrived.
    ///
    /// The default does nothing, and that is **sound rather than lax**: the
    /// default `wait` returns immediately, and a wait that never blocks
    /// trivially returns within any bound. The obligation is the same one
    /// `wait` already carries — a transport that really blocks must implement
    /// this too, or it will hold a caller past a deadline it promised to keep.
    ///
    /// # Errors
    ///
    /// Whatever the transport fails with. A duration the transport cannot
    /// express should be an error rather than a silent rounding: a caller that
    /// asked for a bound and did not get one has no way to find out otherwise.
    fn set_wait_timeout(&mut self, _timeout: Option<Duration>) -> Result<(), Self::Error> {
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
    window_list: Option<WindowList>,
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

    /// Cap how long [`Self::wait`] parks. `None` restores parking until
    /// something arrives.
    ///
    /// See [`Transport::set_wait_timeout`]. This is the seam an event loop uses
    /// to keep a deadline of its own — an animation frame, a blinking caret —
    /// without abandoning the blocking wait for a poll.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the transport rejects the duration.
    pub fn set_wait_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<(), ClientError<T::Error>> {
        self.transport
            .set_wait_timeout(timeout)
            .map_err(ClientError::Transport)
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
            Frame::WindowList(list) => {
                self.window_list = Some(list);
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
        self.window_list.as_ref().map(|l| l.windows.as_slice())
    }

    /// The last `WLST` frame whole — the windows *and* the desktop showing.
    ///
    /// For the caller that needs both, which is any shell drawing a taskbar:
    /// which windows the user can see is the comparison between a window's own
    /// [`WindowInfo::workspace`] and this frame's
    /// [`WindowList::current_workspace`], and taking them from two calls invites
    /// taking them from two different frames.
    #[must_use]
    pub const fn desktop(&self) -> Option<&WindowList> {
        self.window_list.as_ref()
    }

    /// The virtual desktop the compositor says is on screen, as of the last
    /// `WLST` frame.
    ///
    /// **Read this; do not remember it.** The compositor switches desktops on
    /// its own account -- activating a window that is filed elsewhere is a
    /// switch -- so a shell that tracked the number itself would list one
    /// desktop's windows while the screen showed another's.
    ///
    /// `None` before the first frame, for the same reason
    /// [`window_list`](Self::window_list) is: "not told yet" is not "desktop 0".
    #[must_use]
    pub fn current_workspace(&self) -> Option<u32> {
        self.window_list.as_ref().map(|l| l.current_workspace)
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

    /// Act on a window this client does not own — what a taskbar button, an
    /// Alt-Tab switcher and a window menu are made of.
    ///
    /// The window id comes from [`window_list`](Self::window_list); there is no
    /// other way to learn one, which is why this and the subscription are the
    /// same privilege.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the exchange fails, and
    /// [`ClientError::Refused`] if the compositor declined — which for a shell
    /// most often means the window closed between the list it drew from and the
    /// click, and is a normal race rather than a fault.
    pub fn shell_control(
        &mut self,
        window: u64,
        action: ShellControlAction,
    ) -> Result<(), ClientError<T::Error>> {
        self.confirm(RequestBody::ShellControl { window, action })
    }

    /// Show a different virtual desktop.
    ///
    /// A shell's, for the same reason [`shell_control`](Self::shell_control) is:
    /// it acts on every window on the display, most of which the caller does
    /// not own. The compositor picks which window gets the keyboard afterwards,
    /// so nothing here names one — and the result comes back in the next window
    /// list, not from this call.
    ///
    /// *How many* desktops there are is not the compositor's business and it
    /// enforces no bound: a desktop with nothing on it is a legal thing to show.
    /// The count is a user preference, and the shell that owns the preference
    /// is the thing that should refuse to walk past it.
    ///
    /// # Errors
    ///
    /// As [`shell_control`](Self::shell_control).
    pub fn switch_workspace(&mut self, workspace: u32) -> Result<(), ClientError<T::Error>> {
        self.confirm(RequestBody::SwitchWorkspace { workspace })
    }

    /// File a window on a different virtual desktop.
    ///
    /// If it is the one showing the window appears, and if not it disappears —
    /// the compositor decides that, from the number it now holds for the window.
    ///
    /// A window outside `Layer::Normal` — a taskbar, a menu, a lock screen — is
    /// on every desktop, and this stores the number for it without obeying it
    /// rather than refusing. Refusing would make the caller special-case a
    /// distinction it cannot always see: layers change, and a shell iterating
    /// "everything the user has open" would have to filter for a rule it has no
    /// stake in.
    ///
    /// # Errors
    ///
    /// As [`shell_control`](Self::shell_control).
    pub fn set_window_workspace(
        &mut self,
        window: u64,
        workspace: u32,
    ) -> Result<(), ClientError<T::Error>> {
        self.confirm(RequestBody::SetWindowWorkspace { window, workspace })
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
            ResponseBody::Ok | ResponseBody::Display(_) | ResponseBody::WorkArea { .. } => {
                Err(ClientError::Mismatched)
            }
        }
    }

    /// Reserve a strip along one edge of the monitor `window` is on, and get
    /// back the work area that leaves.
    ///
    /// This is the call a taskbar or dock makes so that tiled windows stop
    /// short of it. A `size` of zero releases a reservation made earlier. The
    /// area returned is what the compositor actually granted, which may be less
    /// than was asked for — see
    /// [`RequestBody::ReserveEdge`](crate::control::RequestBody::ReserveEdge).
    ///
    /// # Errors
    ///
    /// As [`Self::round_trip`], plus [`ClientError::Refused`] if the compositor
    /// declined — which for this request means the window is not the caller's
    /// or the caller is not entitled to reserve at all — and
    /// [`ClientError::Mismatched`] if it answered with something other than an
    /// area.
    pub fn reserve_edge(
        &mut self,
        window: u64,
        edge: crate::reserve::PanelEdge,
        size: u32,
    ) -> Result<(i32, i32, u32, u32), ClientError<T::Error>> {
        match self.round_trip(RequestBody::ReserveEdge { window, edge, size })? {
            ResponseBody::WorkArea {
                x,
                y,
                width,
                height,
            } => Ok((x, y, width, height)),
            ResponseBody::Error { message } => Err(ClientError::Refused(message)),
            ResponseBody::Ok | ResponseBody::WindowCreated { .. } | ResponseBody::Display(_) => {
                Err(ClientError::Mismatched)
            }
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
            ResponseBody::WindowCreated { .. }
            | ResponseBody::Display(_)
            | ResponseBody::WorkArea { .. } => Err(ClientError::Mismatched),
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
    use guitk::event::Event;

    use super::*;
    use crate::WindowSpec;
    use crate::control::encode_responses;
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
