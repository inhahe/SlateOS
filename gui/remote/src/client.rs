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

use guitk::event::Event;
use guitk::render::RenderTree;

use crate::input::{InputEvent, try_decode_input_frame};
use crate::{DecodeError, encode_frame};

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
}

impl<E: core::fmt::Display> core::fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
        }
    }
}

impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for ClientError<E> {}

/// Drives one window: reads events, dispatches them, sends frames.
pub struct Client<T: Transport> {
    transport: T,
    window: u64,
    width: u32,
    height: u32,
    /// Bytes read but not yet a whole frame. A stream does not respect frame
    /// boundaries, so this is where a frame split across two reads waits for
    /// its remainder.
    inbox: Vec<u8>,
    /// Reused across frames so a steady-state redraw allocates nothing.
    outbox: Vec<u8>,
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
    /// Bind a transport to one window of a known size.
    pub fn new(transport: T, window: u64, width: u32, height: u32) -> Self {
        Self {
            transport,
            window,
            width,
            height,
            inbox: Vec::new(),
            outbox: Vec::new(),
            needs_first_paint: true,
            focused: false,
            exiting: false,
            stray_events: 0,
        }
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
        while self.transport.is_open() {
            if !self.tick(app)? {
                break;
            }
            self.transport.wait().map_err(ClientError::Transport)?;
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
        self.outbox.clear();
        encode_frame(&tree, &mut self.outbox);
        self.transport
            .write(&self.outbox)
            .map_err(ClientError::Transport)?;
        self.needs_first_paint = false;
        Ok(())
    }

    /// Read whatever is available and decode every complete frame in it.
    fn receive(&mut self) -> Result<Vec<InputEvent>, ClientError<T::Error>> {
        self.transport
            .read(&mut self.inbox)
            .map_err(ClientError::Transport)?;

        let mut events = Vec::new();
        let mut consumed = 0usize;
        loop {
            let rest = self.inbox.get(consumed..).unwrap_or(&[]);
            match try_decode_input_frame(rest).map_err(ClientError::Protocol)? {
                Some((mut batch, used)) => {
                    // A zero-length frame would spin here forever. It cannot
                    // happen — every frame is at least a header — but the loop
                    // should not depend on that being true of a *remote*
                    // encoder we do not control.
                    if used == 0 {
                        break;
                    }
                    events.append(&mut batch);
                    consumed += used;
                }
                // A partial frame: leave it in the buffer for the next read.
                None => break,
            }
        }
        // Drained from the front rather than reallocated, so a steady stream
        // reuses one buffer instead of copying the remainder each tick.
        self.inbox.drain(..consumed);
        Ok(events)
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
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use guitk::color::Color;
    use guitk::event::{Key, KeyEvent, Modifiers, MouseEvent, MouseEventKind};

    use super::*;
    use crate::decode_frame;
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
        assert_eq!(c.transport.sent.len(), 1);
    }

    #[test]
    fn a_quiet_tick_after_the_first_paint_sends_nothing() {
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        c.tick(&mut app).unwrap();
        assert_eq!(app.renders.len(), 1, "an idle app must not redraw");
        assert_eq!(c.transport.sent.len(), 1);
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
    fn what_is_sent_is_a_decodable_draw_frame() {
        // The other half of the loop: a client's output must be exactly what a
        // compositor's decoder accepts.
        let mut c = client(vec![]);
        let mut app = RecordingApp::default();
        c.tick(&mut app).unwrap();
        let sent = &c.transport.sent[0];
        let (tree, used) = decode_frame(sent).unwrap();
        assert_eq!(used, sent.len());
        assert_eq!(tree.commands.len(), 1);
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
        assert!(!c.transport.is_open());
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
            c.transport.waits, 1,
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
        assert_eq!(c.transport.sent.len(), 2);
    }
}
