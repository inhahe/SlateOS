//! The strap between an application and its window, written once.
//!
//! There are ~140 application crates in this tree and, at the time of writing,
//! exactly one of them — `apps/editor` — was actually connected to a
//! compositor. See `known-issues.md` → `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`.
//! The obvious repair is to give each app an event loop; that is the wrong
//! repair, and its own step (d) says so: 140 hand-written loops are 140 chances
//! to get the seam subtly different, and the differences are exactly the sort
//! nobody notices until two applications behave unlike each other for no reason
//! a user can name.
//!
//! So the strap lives here, once, and an application supplies only the part
//! that is genuinely its own: what to draw, and what to do with an event.
//!
//! ## What the strap actually is
//!
//! Read `apps/editor`'s `main` before this module and the list is concrete. It
//! parses `--display`, connects, turns a refused connection into a diagnostic
//! that says *no compositor is running* rather than printing an errno, builds
//! one window, submits the first frame — which no event will ever ask for,
//! because nothing has happened yet — runs the loop, and carries a failed
//! `submit` out of the handler by hand because the handler returns
//! [`EventResponse`] rather than a `Result`. That is ~120 lines, none of it
//! about editing text, and every one of them a line the other 139 apps would
//! otherwise write again.
//!
//! ## Why here and not in `guiremote`
//!
//! It was in `guiremote`, as `guiremote::client::Client`, and this module
//! replaces it. That version was written before this crate had an event loop at
//! all and was superseded within the week; it then sat with **no production
//! caller of its own** — the very defect it was built to cure, and lesson 45
//! (`a feature with no production caller is a feature that does not exist`)
//! arriving inside the cure. Two loops for one job is also the tree's own
//! "three enums for one concept" failure in a new place.
//!
//! The surviving loop must be this one, because [`EventLoop`] owns the frame
//! clock. `Client` could not tick: it had no wake-up list, so it parked in
//! `Connection::wait` until input arrived, and an application with an animation
//! and an idle user simply stopped. A harness that cannot deliver
//! [`Event::Tick`] is a harness that reproduces lesson 47 — *an app that keeps
//! time but never receives the clock* — for every app built on it.
//!
//! [`Response`] and [`App`] moved here unchanged, so the vocabulary is the same
//! one; only the driver underneath them is different.
//!
//! ## Usage
//!
//! ```rust
//! use oswindow::app::{self, App, Response};
//! use oswindow::{Event, RenderTree};
//!
//! struct Clock {
//!     seconds: u64,
//! }
//!
//! impl App for Clock {
//!     fn title(&self) -> String {
//!         "Clock".to_string()
//!     }
//!
//!     // An app that keeps time says so here, and the harness delivers the
//!     // clock. Left at the default `None`, no `Event::Tick` ever arrives --
//!     // which is correct for an app that has nothing to advance, and is the
//!     // one thing to get right for an app that does.
//!     fn tick_interval(&self) -> Option<std::time::Duration> {
//!         Some(std::time::Duration::from_millis(500))
//!     }
//!
//!     fn on_event(&mut self, event: &Event) -> Response {
//!         match *event {
//!             Event::Tick { elapsed_ms } => {
//!                 self.seconds = self.seconds.saturating_add(elapsed_ms / 1000);
//!                 Response::Redraw
//!             }
//!             _ => Response::Idle,
//!         }
//!     }
//!
//!     fn render(&mut self, _width: f32, _height: f32) -> RenderTree {
//!         RenderTree::new()
//!     }
//! }
//!
//! # if false {
//! fn main() -> std::process::ExitCode {
//!     app::launch("clock", &mut Clock { seconds: 0 })
//! }
//! # }
//! ```

use std::process::ExitCode;
use std::time::Duration;

use guiremote::client::Transport;

use crate::{
    DISPLAY_VAR, Dispatch, Error, Event, EventLoop, EventResponse, Link, RenderTree, Window,
    WindowBuilder,
};

// ---------------------------------------------------------------------------
// What an application supplies
// ---------------------------------------------------------------------------

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
    /// Close down: the loop finishes after this event.
    Exit,
}

/// The part of an application the toolkit cannot write for it.
///
/// Everything with a default is a decision an application is entitled not to
/// have an opinion about. The two without — [`App::on_event`] and
/// [`App::render`] — are the application itself.
pub trait App {
    /// What to put in the title bar.
    ///
    /// Read once, when the window is created. An application that retitles
    /// itself as its document changes does so through
    /// [`EventLoop::window_mut`]; making this live would mean re-reading it on
    /// every event to find out whether it had changed, which is a round trip
    /// per mouse move to answer "no".
    fn title(&self) -> String;

    /// How big to ask for the window to be, in pixels.
    ///
    /// A request, not a guarantee: the compositor may give a different size and
    /// will say so with [`Event::Resize`] before the first frame is drawn — so
    /// [`App::render`] must use the width and height it is handed rather than
    /// these.
    fn initial_size(&self) -> (u32, u32) {
        (800, 600)
    }

    /// Whether the user may resize the window.
    fn resizable(&self) -> bool {
        true
    }

    /// How often to be sent an [`Event::Tick`], or `None` never to be.
    ///
    /// **This is the one method to get right in an application that measures
    /// time.** An app that ages anything — a stopwatch, a metronome, a toast
    /// that expires, a progress bar — and leaves this at `None` receives no
    /// clock, and the state it ages is frozen for the life of the process while
    /// the window still lays out, still repaints, still answers the keyboard,
    /// and still shows a plausible zero. Five applications in this tree shipped
    /// exactly that, all five with passing tests over the frozen code:
    /// `known-issues.md` lesson 47. `scripts/check-tick-wiring.py` is the gate
    /// that now catches it.
    ///
    /// Consulted after every event, not only after a tick, so an application
    /// may start and stop its clock as the user starts and stops whatever is
    /// moving. That matters in both directions: an app whose clock is off has
    /// no tick to re-arm from, and an app that keeps ticking with nothing to
    /// advance holds the whole desktop awake.
    ///
    /// The interval is a floor rather than a promise. Ticks are delivered when
    /// the loop next runs, and [`Event::Tick`] carries the interval that
    /// *actually* elapsed — which is why an application must advance by
    /// `elapsed_ms` and never by the value it asked for here.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    /// React to one event.
    ///
    /// Return [`Response::Redraw`] only when something visible actually
    /// changed. Returning it unconditionally is the easy mistake and costs a
    /// full frame per mouse move.
    fn on_event(&mut self, event: &Event) -> Response;

    /// Draw the current state at the current window size.
    ///
    /// The size is the one the compositor last reported, so a frame drawn
    /// during a resize is drawn at the new size rather than the previous one —
    /// a frame at the old size is a visibly stretched or clipped window for one
    /// refresh.
    fn render(&mut self, width: f32, height: f32) -> RenderTree;
}

// ---------------------------------------------------------------------------
// Driving one
// ---------------------------------------------------------------------------

/// Create the window an application asks for and return its id.
///
/// Separate from [`drive`] so that a caller with more than one window, or one
/// that wants to ask the compositor something before deciding how big to be,
/// can still use the loop below for the window it does have.
///
/// # Errors
///
/// As [`EventLoop::create`]: the compositor refused, or went away first.
pub fn open<T: Transport, A: App + ?Sized>(
    events: &mut EventLoop<T>,
    app: &A,
) -> Result<u64, Error<T>> {
    let (width, height) = app.initial_size();
    WindowBuilder::new(app.title(), width, height)
        .resizable(app.resizable())
        .build(events)
}

/// Drive an application on an open window until it exits or the link closes.
///
/// Submits the first frame before waiting for anything, because no event is
/// going to ask for it: nothing has happened yet, and a window that has never
/// been drawn is blank.
///
/// Events for other windows are passed over rather than dispatched, so a
/// process that owns a second window elsewhere does not feed its events to this
/// application.
///
/// At most one frame is drawn per batch of events, however many of them asked
/// for a redraw. A mouse drag arrives as a burst of thirty moves, and an
/// application that drew per event would send twenty-nine frames that were
/// already stale when they went out. See [`EventLoop::run_batched`].
///
/// # Errors
///
/// As [`EventLoop::run_batched`], plus any failure to submit a frame. A failed
/// submit stops the loop rather than being swallowed: an application that ran
/// on happily while the screen no longer changed would be the frozen-clock
/// defect again, one layer down.
pub fn drive<T: Transport, A: App + ?Sized>(
    events: &mut EventLoop<T>,
    window: u64,
    app: &mut A,
) -> Result<(), Error<T>> {
    // Nothing has happened yet, so no event is going to ask for the first
    // frame, and a window that has never been drawn is blank.
    let (width, height) = client_size(events, window, app);
    let first = app.render(width, height);
    events.submit(window, &first)?;
    // Armed before the first park, so an application whose clock is its only
    // input still receives one. Deferring this to the first `Settled` would
    // mean a stopwatch nobody touched never started.
    sync_clock(events, window, app);

    // A failed submit has nowhere to be reported from inside the handler — its
    // return type is the loop's `EventResponse`, not a `Result` — so it is
    // carried out here and the loop stopped.
    let mut failure = None;
    let mut dirty = false;
    events.run_batched(|events, dispatch| match dispatch {
        Dispatch::Event {
            window: id,
            ref event,
        } => {
            if id != window {
                return EventResponse::Continue;
            }
            // A geometry change invalidates the last frame no matter what the
            // application makes of it: the frame on screen was drawn for the
            // old size, and every application would otherwise have to remember
            // to say `Redraw` here. That is 137 chances to forget, and the
            // symptom of forgetting — a window that shows a stale frame
            // stretched or letterboxed until something else happens to be
            // clicked — is exactly the kind nobody attributes to the resize.
            // So the loop takes the decision, as it does the batch boundary.
            if matches!(event, Event::Resize { .. } | Event::ScaleChanged { .. }) {
                dirty = true;
            }
            match app.on_event(event) {
                Response::Idle => EventResponse::Continue,
                Response::Redraw => {
                    dirty = true;
                    EventResponse::Continue
                }
                Response::Exit => EventResponse::Exit,
            }
        }
        Dispatch::Settled => {
            // Re-armed at every batch boundary rather than only after a tick,
            // so that an application can start animating in response to a key
            // press. The frame clock is one-shot by design (see
            // `EventLoop::wake_at`), which makes *stopping* the default; this
            // restores "keep going" as the default for an application that has
            // declared an interval, without giving up the property that an
            // idle desktop parks for ever.
            sync_clock(events, window, app);
            if !std::mem::take(&mut dirty) {
                return EventResponse::Continue;
            }
            let (width, height) = client_size(events, window, app);
            let tree = app.render(width, height);
            if let Err(e) = events.submit(window, &tree) {
                failure = Some(e);
                return EventResponse::Exit;
            }
            EventResponse::Continue
        }
    })?;

    failure.map_or(Ok(()), Err)
}

/// Bring a window's wake-up into agreement with what the application now wants.
fn sync_clock<T: Transport, A: App + ?Sized>(events: &mut EventLoop<T>, window: u64, app: &A) {
    match app.tick_interval() {
        Some(interval) if !events.is_waking(window) => events.wake_after(window, interval),
        // Already armed: leave the existing deadline alone. Re-arming here
        // would push the next tick further away with every event, so an
        // application would animate only while the pointer was still — the
        // frozen-clock defect wearing a subtler coat, since it would look
        // correct in every test that does not move the mouse.
        Some(_) => {}
        None => events.cancel_wake(window),
    }
}

/// The size to draw at: what the compositor last reported, or what was asked
/// for if it has not reported yet.
fn client_size<T: Transport, A: App + ?Sized>(
    events: &EventLoop<T>,
    window: u64,
    app: &A,
) -> (f32, f32) {
    let (width, height) = events
        .window(window)
        .map_or_else(|| app.initial_size(), Window::size);
    // A window wider than 16.7 million pixels would lose precision here. The
    // render vocabulary is `f32` throughout, so the cast happens somewhere
    // regardless, and this is the narrowest place to put it.
    #[allow(clippy::cast_precision_loss)]
    (width as f32, height as f32)
}

// ---------------------------------------------------------------------------
// The whole strap
// ---------------------------------------------------------------------------

/// The command-line arguments an application shares with every other one.
///
/// `--display ADDR` overrides `SLATE_DISPLAY`, in the same way and for the same
/// reason a compositor takes an address as its first argument: a second display
/// on one machine should not require editing the environment of the first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// The address given with `--display`, if any.
    pub display: Option<String>,
    /// Everything that was not a display option, in order — file names,
    /// usually. An application that takes no arguments should say so if this is
    /// not empty rather than ignoring it.
    pub rest: Vec<String>,
}

impl Args {
    /// Split arguments into a display address and everything else.
    ///
    /// A lone `--` ends option parsing, so a file genuinely named `--display`
    /// is still openable.
    ///
    /// # Errors
    ///
    /// Returns a message fit to print if `--display` is given without an
    /// address.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut display = None;
        let mut rest = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--" => {
                    rest.extend(args);
                    break;
                }
                "--display" => {
                    display = Some(args.next().ok_or_else(|| {
                        "--display needs an address, e.g. --display 127.0.0.1:7373".to_string()
                    })?);
                }
                other => match other.strip_prefix("--display=") {
                    Some(addr) => display = Some(addr.to_string()),
                    None => rest.push(arg),
                },
            }
        }
        Ok(Self { display, rest })
    }

    /// [`Args::parse`] over this process's own arguments, less argv[0].
    ///
    /// # Errors
    ///
    /// As [`Args::parse`].
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1))
    }
}

/// Connect to the compositor, reporting a failure in terms a user can act on.
///
/// The raw error is almost always "connection refused", which means simply that
/// no compositor is running — but the errno alone reads like a fault in the
/// application, so the diagnostic says what it means and what to do about it.
///
/// # Errors
///
/// The message has already been printed to stderr; the [`ExitCode`] is what to
/// return from `main`.
fn dial(program: &str, display: Option<&str>) -> Result<Link, ExitCode> {
    let dialled = match display {
        Some(addr) => crate::connect_to(addr),
        None => crate::connect(),
    };
    dialled.map_err(|e| {
        eprintln!("{program}: cannot reach the compositor: {e}");
        eprintln!("  A compositor must be running for {program} to have a window.");
        eprintln!("  Start one with `compositor`, or point {program} at an existing");
        eprintln!("  display with `--display HOST:PORT` or the {DISPLAY_VAR} variable.");
        ExitCode::FAILURE
    })
}

/// Everything between `fn main` and the application: parse, connect, open one
/// window, run until it is done.
///
/// This is the whole of what an ordinary single-window application's `main`
/// should contain:
///
/// ```rust,ignore
/// fn main() -> std::process::ExitCode {
///     oswindow::app::launch("metronome", &mut MetronomeApp::new())
/// }
/// ```
///
/// Diagnostics are printed here rather than returned, because there is nothing
/// above `main` to report them to and every application would otherwise print
/// the same four lines slightly differently.
///
/// An application that needs the non-display arguments should call
/// [`Args::from_env`] itself, construct from them, and then call
/// [`launch_with`] with the display it parsed — `launch` re-parses argv only so
/// that an application with no arguments of its own needs no ceremony at all.
pub fn launch<A: App + ?Sized>(program: &str, app: &mut A) -> ExitCode {
    let args = match Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{program}: {e}");
            // Distinct from `FAILURE`: the command line was wrong, which is a
            // different thing for a script to react to than a compositor that
            // is not running.
            return ExitCode::from(2);
        }
    };
    if let Some(unexpected) = args.rest.first() {
        eprintln!("{program}: unexpected argument `{unexpected}`");
        return ExitCode::from(2);
    }
    launch_with(program, args.display.as_deref(), app)
}

/// [`launch`] with the display address supplied, for an application that has
/// already parsed its own command line.
pub fn launch_with<A: App + ?Sized>(program: &str, display: Option<&str>, app: &mut A) -> ExitCode {
    let transport = match dial(program, display) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let mut events = EventLoop::new(transport);
    let window = match open(&mut events, app) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{program}: the compositor refused the window: {e}");
            return ExitCode::FAILURE;
        }
    };
    match drive(&mut events, window, app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: the connection failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::InputEvent;
    use crate::testing::{TestConnection, desktop};

    /// An application that records what it was asked, and answers as told.
    struct Recorder {
        seen: Rc<RefCell<Vec<Event>>>,
        /// The sizes `render` was called at, in order.
        drawn: Rc<RefCell<Vec<(f32, f32)>>>,
        answer: Response,
        interval: Option<Duration>,
    }

    impl Recorder {
        fn new(answer: Response) -> Self {
            Self {
                seen: Rc::new(RefCell::new(Vec::new())),
                drawn: Rc::new(RefCell::new(Vec::new())),
                answer,
                interval: None,
            }
        }

        fn ticking(mut self, every: Duration) -> Self {
            self.interval = Some(every);
            self
        }
    }

    impl App for Recorder {
        fn title(&self) -> String {
            "Recorder".to_string()
        }

        fn initial_size(&self) -> (u32, u32) {
            (640, 480)
        }

        fn tick_interval(&self) -> Option<Duration> {
            self.interval
        }

        fn on_event(&mut self, event: &Event) -> Response {
            self.seen.borrow_mut().push(event.clone());
            self.answer
        }

        fn render(&mut self, width: f32, height: f32) -> RenderTree {
            self.drawn.borrow_mut().push((width, height));
            RenderTree::new()
        }
    }

    fn opened(app: &Recorder) -> (EventLoop<TestConnection>, u64) {
        let (mut events, _desktop) = desktop();
        let window = open(&mut events, app).expect("the compositor should have granted it");
        (events, window)
    }

    #[test]
    fn the_window_is_opened_on_the_apps_own_terms() {
        let app = Recorder::new(Response::Idle);
        let (events, window) = opened(&app);
        let w = events.window(window).expect("the loop should know it");
        assert_eq!(w.title(), "Recorder");
        assert_eq!(w.size(), (640, 480));
        assert!(w.is_resizable(), "the default is resizable");
    }

    /// The first frame is the one no event asks for: nothing has happened yet,
    /// so an app drawn only on demand would show a blank window until the user
    /// touched it.
    #[test]
    fn the_first_frame_is_submitted_before_anything_happens() {
        let mut app = Recorder::new(Response::Exit);
        let (mut events, window) = opened(&app);
        let drawn = Rc::clone(&app.drawn);

        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            drawn.borrow().first(),
            Some(&(640.0, 480.0)),
            "the first frame must be drawn at the window's size, unprompted"
        );
    }

    #[test]
    fn idle_does_not_cost_a_frame() {
        let mut app = Recorder::new(Response::Idle);
        let (mut events, window) = opened(&app);
        let drawn = Rc::clone(&app.drawn);

        events.inject_event(window, Event::FocusIn);
        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            drawn.borrow().len(),
            1,
            "only the unprompted first frame; Idle asked for nothing"
        );
    }

    /// Coalescing, which is the whole reason [`EventLoop::run_batched`] exists.
    ///
    /// A mouse drag arrives as one burst, and an application that drew per
    /// event would send a frame for each — every one but the last already stale
    /// when it went out. Scripted through the desktop rather than injected,
    /// because a batch boundary is what is under test and
    /// [`EventLoop::inject_event`] puts everything in one.
    #[test]
    fn a_burst_of_changes_costs_one_frame_and_not_one_each() {
        let mut app = Recorder::new(Response::Redraw);
        let (mut events, desktop) = desktop();
        let window = open(&mut events, &app).expect("granted");
        let drawn = Rc::clone(&app.drawn);
        let seen = Rc::clone(&app.seen);

        desktop.borrow_mut().script.push_back(vec![
            InputEvent::new(window, Event::FocusIn),
            InputEvent::new(window, Event::FocusOut),
            InputEvent::new(window, Event::FocusIn),
        ]);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(seen.borrow().len(), 3, "all three should have been seen");
        assert_eq!(
            drawn.borrow().len(),
            2,
            "the unprompted first frame, and one for the settled burst — a \
             count of 4 means every event drew, which is 3 stale frames per drag"
        );
    }

    /// A frame drawn at the previous size is a stretched or clipped window for
    /// one refresh, so the resize must be folded in before the render that
    /// answers it.
    #[test]
    fn a_resize_is_drawn_at_the_new_size() {
        let mut app = Recorder::new(Response::Redraw);
        let (mut events, desktop) = desktop();
        let window = open(&mut events, &app).expect("granted");
        let drawn = Rc::clone(&app.drawn);

        desktop.borrow_mut().script.push_back(vec![InputEvent::new(
            window,
            Event::Resize {
                width: 1024,
                height: 768,
            },
        )]);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            drawn.borrow().last(),
            Some(&(1024.0, 768.0)),
            "the frame answering the resize was drawn at the old size"
        );
    }

    /// The resize redraw is the loop's decision, not the application's.
    ///
    /// An application that answers `Idle` to everything is not saying "the
    /// window still looks right at the new size" — it is saying "nothing in my
    /// model changed", which is true and beside the point. Left to each app
    /// this is 137 chances to forget, and forgetting shows a stale frame at the
    /// wrong size until some later event happens to redraw.
    #[test]
    fn a_resize_redraws_even_an_app_that_says_it_is_idle() {
        let mut app = Recorder::new(Response::Idle);
        let (mut events, desktop) = desktop();
        let window = open(&mut events, &app).expect("granted");
        let drawn = Rc::clone(&app.drawn);

        desktop.borrow_mut().script.push_back(vec![InputEvent::new(
            window,
            // Deliberately not `Recorder`'s own 640x480: a "new" size equal to
            // the old one is a resize that needs no frame, so the assertion
            // would hold whether or not the rule existed.
            Event::Resize {
                width: 320,
                height: 200,
            },
        )]);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            drawn.borrow().last(),
            Some(&(320.0, 200.0)),
            "an idle verdict suppressed the frame the resize itself required"
        );
    }

    /// Another window's events are not this application's business.
    #[test]
    fn events_for_another_window_are_not_dispatched() {
        let mut app = Recorder::new(Response::Redraw);
        let (mut events, window) = opened(&app);
        let seen = Rc::clone(&app.seen);

        // A second window on the same connection, so the loop can route to it.
        let other = WindowBuilder::new("Other", 100, 100)
            .build(&mut events)
            .expect("granted");
        events.inject_event(other, Event::FocusIn);
        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            seen.borrow().as_slice(),
            &[Event::CloseRequested],
            "the other window's focus event reached this app"
        );
    }

    /// An application that stops after it has been ticked `limit` times.
    ///
    /// The counter is what bounds [`the_clock_keeps_running_without_input`]: a
    /// zero-length interval makes the loop deliver ticks as fast as it can, and
    /// the app is what says when enough have arrived.
    struct Ticker {
        ticks: u32,
        limit: u32,
        elapsed_seen: Vec<u64>,
    }

    impl App for Ticker {
        fn title(&self) -> String {
            "Ticker".to_string()
        }

        fn tick_interval(&self) -> Option<Duration> {
            // Already due when armed, so the loop delivers on its next pass
            // rather than parking. A real application asks for the interval it
            // actually wants; this one is testing the re-arm, not the parking,
            // and a test that has to sleep to observe a rule is a test that
            // will be flaky about it.
            Some(Duration::ZERO)
        }

        fn on_event(&mut self, event: &Event) -> Response {
            if let Event::Tick { elapsed_ms } = *event {
                self.elapsed_seen.push(elapsed_ms);
                self.ticks = self.ticks.saturating_add(1);
                if self.ticks >= self.limit {
                    return Response::Exit;
                }
            }
            Response::Idle
        }

        fn render(&mut self, _width: f32, _height: f32) -> RenderTree {
            RenderTree::new()
        }
    }

    /// The whole point of the harness over `guiremote::client::Client`, which
    /// had no wake-up list and so parked until input arrived.
    ///
    /// The frame clock is one-shot by design, so a harness that arms it once
    /// and never again delivers exactly one tick and then goes silent — lesson
    /// 47 in the form a half-working harness takes, and invisible to any test
    /// that only checks the first tick arrives. Hence *five*: without the
    /// re-arm the loop would take its one tick, park in `wait`, find the
    /// scripted desktop has nothing further to say, and end at one.
    #[test]
    fn the_clock_keeps_running_without_input() {
        let mut app = Ticker {
            ticks: 0,
            limit: 5,
            elapsed_seen: Vec::new(),
        };
        let (mut events, _desktop) = desktop();
        let window = open(&mut events, &app).expect("granted");

        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(
            app.ticks, 5,
            "the clock stopped after the first tick: nothing re-armed it, so \
             every app built on this harness would freeze while still \
             repainting"
        );
        assert_eq!(
            app.elapsed_seen.len(),
            5,
            "every tick must carry an interval for the app to advance by"
        );
    }

    /// An app that never declares an interval must never be woken, or an idle
    /// desktop burns a core discovering it has nothing to draw.
    #[test]
    fn an_app_that_asks_for_no_clock_is_not_given_one() {
        let app = Recorder::new(Response::Idle);
        let (mut events, window) = opened(&app);
        sync_clock(&mut events, window, &app);
        assert!(!events.is_waking(window));
    }

    /// The other direction: an app that stops asking must stop being woken.
    /// Consulted after *every* event rather than only after a tick, so this
    /// also covers an app that starts its clock in response to a key press —
    /// which has no tick to re-arm from.
    #[test]
    fn the_interval_may_be_taken_up_and_put_down() {
        let mut app = Recorder::new(Response::Idle);
        let (mut events, window) = opened(&app);

        sync_clock(&mut events, window, &app);
        assert!(!events.is_waking(window), "asked for no clock, got one");

        app.interval = Some(Duration::from_millis(1));
        sync_clock(&mut events, window, &app);
        assert!(
            events.is_waking(window),
            "an app that started animating was left without a clock, and has \
             no tick to re-arm from"
        );

        app.interval = None;
        sync_clock(&mut events, window, &app);
        assert!(
            !events.is_waking(window),
            "the wake-up outlived the animation that wanted it"
        );
    }

    /// Re-arming on every event would push the deadline further away with each
    /// mouse move, so an app would animate only while the pointer was still.
    #[test]
    fn an_armed_clock_is_not_pushed_back_by_other_events() {
        let app = Recorder::new(Response::Idle).ticking(Duration::from_secs(10));
        let (mut events, window) = opened(&app);
        sync_clock(&mut events, window, &app);
        let first = events.next_wakeup().expect("armed");

        sync_clock(&mut events, window, &app);
        assert_eq!(
            events.next_wakeup(),
            Some(first),
            "the deadline moved, so a steady stream of input would starve the \
             clock indefinitely"
        );
    }

    #[test]
    fn exit_stops_the_loop_without_drawing_again() {
        let mut app = Recorder::new(Response::Exit);
        let (mut events, window) = opened(&app);
        let drawn = Rc::clone(&app.drawn);
        let seen = Rc::clone(&app.seen);

        events.inject_event(window, Event::FocusIn);
        events.inject_event(window, Event::FocusOut);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(seen.borrow().len(), 1, "the loop kept going after Exit");
        assert_eq!(drawn.borrow().len(), 1, "Exit should not draw a frame");
    }

    /// A close request the application does not act on still closes the window.
    /// A title-bar X that does nothing is worse than an app that quits when it
    /// would rather not have.
    #[test]
    fn a_close_request_closes_even_when_the_app_says_idle() {
        let mut app = Recorder::new(Response::Idle);
        let (mut events, window) = opened(&app);

        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");
        assert!(!events.is_running());
    }

    /// A transport that stops accepting bytes when the flag is set.
    ///
    /// The loopback pipe cannot fail — its error type is [`Infallible`] — so a
    /// harness built only on it can never exercise the one path where a failure
    /// has nowhere to be reported from. That is exactly the path worth
    /// pinning, so this wraps it in something that can.
    ///
    /// [`Infallible`]: std::convert::Infallible
    struct Breakable {
        inner: TestConnection,
        broken: Rc<std::cell::Cell<bool>>,
    }

    impl crate::ConnectionTransport for Breakable {
        type Error = &'static str;

        fn read(&mut self, buf: &mut Vec<u8>) -> Result<usize, Self::Error> {
            self.inner.read(buf).map_err(|e| match e {})
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            if self.broken.get() {
                return Err("the compositor went away");
            }
            self.inner.write(bytes).map_err(|e| match e {})
        }

        fn is_open(&self) -> bool {
            self.inner.is_open()
        }

        fn wait(&mut self) -> Result<(), Self::Error> {
            self.inner.wait().map_err(|e| match e {})
        }

        fn set_wait_timeout(&mut self, timeout: Option<Duration>) -> Result<(), Self::Error> {
            self.inner.set_wait_timeout(timeout).map_err(|e| match e {})
        }
    }

    /// An application that breaks the link the moment it is asked to redraw.
    ///
    /// Breaking it from *inside* `on_event` is what puts the failure in the
    /// handler, which is the only place it has nowhere to be returned to.
    struct Saboteur {
        broken: Rc<std::cell::Cell<bool>>,
        events_seen: u32,
    }

    impl App for Saboteur {
        fn title(&self) -> String {
            "Saboteur".to_string()
        }

        fn on_event(&mut self, _event: &Event) -> Response {
            self.events_seen = self.events_seen.saturating_add(1);
            self.broken.set(true);
            Response::Redraw
        }

        fn render(&mut self, _width: f32, _height: f32) -> RenderTree {
            RenderTree::new()
        }
    }

    /// A failed submit must stop the loop. Swallowed, it leaves an application
    /// running happily while the screen no longer changes — which is the frozen
    /// clock one layer down, and just as invisible.
    #[test]
    fn a_failed_submit_is_carried_out_of_the_handler() {
        let broken = Rc::new(std::cell::Cell::new(false));
        let (client_end, server_end) = crate::pipe();
        let server = Rc::new(RefCell::new(crate::testing::TestDesktop::new(server_end)));
        let mut events = EventLoop::new(Breakable {
            inner: TestConnection {
                pipe: client_end,
                server: Rc::clone(&server),
                asked: Vec::new(),
            },
            broken: Rc::clone(&broken),
        });

        let mut app = Saboteur {
            broken: Rc::clone(&broken),
            events_seen: 0,
        };
        let window = open(&mut events, &app).expect("granted");

        // Two batches, so a loop that swallowed the failure would go on to
        // dispatch the second — which is what the count below rules out.
        let mut desk = server.borrow_mut();
        desk.script
            .push_back(vec![InputEvent::new(window, Event::FocusIn)]);
        desk.script
            .push_back(vec![InputEvent::new(window, Event::FocusOut)]);
        drop(desk);

        let outcome = drive(&mut events, window, &mut app);
        assert!(
            outcome.is_err(),
            "a frame that never reached the compositor was reported as success"
        );
        assert_eq!(
            app.events_seen, 1,
            "the loop kept dispatching after the screen stopped updating — an \
             application running on happily over a dead link"
        );
    }

    // -- Arguments ----------------------------------------------------------

    #[test]
    fn a_bare_command_line_has_no_display_and_no_arguments() {
        let args = Args::parse(Vec::new()).expect("nothing to reject");
        assert_eq!(args, Args::default());
    }

    #[test]
    fn display_is_taken_in_both_spellings() {
        let spaced = Args::parse(["--display".into(), "127.0.0.1:7373".into()]).unwrap();
        let joined = Args::parse(["--display=127.0.0.1:7373".to_string()]).unwrap();
        assert_eq!(spaced.display.as_deref(), Some("127.0.0.1:7373"));
        assert_eq!(spaced, joined);
    }

    #[test]
    fn a_missing_address_is_a_message_and_not_a_panic() {
        let e = Args::parse(["--display".to_string()]).unwrap_err();
        assert!(e.contains("--display needs an address"), "{e}");
    }

    /// A file genuinely named `--display` is still openable.
    #[test]
    fn a_double_dash_ends_option_parsing() {
        let args = Args::parse(["--".into(), "--display".into(), "notes.txt".into()]).unwrap();
        assert_eq!(args.display, None);
        assert_eq!(args.rest, vec!["--display".to_string(), "notes.txt".into()]);
    }

    #[test]
    fn everything_else_is_kept_in_order() {
        let args = Args::parse([
            "a.txt".to_string(),
            "--display".into(),
            "x:1".into(),
            "b.txt".into(),
        ])
        .unwrap();
        assert_eq!(args.display.as_deref(), Some("x:1"));
        assert_eq!(args.rest, vec!["a.txt".to_string(), "b.txt".into()]);
    }

    /// The harness must not silently accept an event it cannot route.
    #[test]
    fn an_event_for_no_window_at_all_is_not_dispatched() {
        let mut app = Recorder::new(Response::Redraw);
        let (mut events, window) = opened(&app);
        let seen = Rc::clone(&app.seen);

        events.inject_event(u64::MAX, Event::FocusIn);
        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(seen.borrow().as_slice(), &[Event::CloseRequested]);
        assert_eq!(
            events.unrouted_events(),
            1,
            "the stray event was not counted"
        );
    }

    /// Guards the shape of the doc example above: an `InputEvent` is what the
    /// wire carries, and the harness must accept one addressed to its window.
    #[test]
    fn the_wire_form_of_an_event_reaches_the_app() {
        let mut app = Recorder::new(Response::Idle);
        let (mut events, window) = opened(&app);
        let seen = Rc::clone(&app.seen);

        let wire = InputEvent::new(window, Event::Tick { elapsed_ms: 7 });
        events.inject_event(wire.window, wire.event.clone());
        events.inject_event(window, Event::CloseRequested);
        drive(&mut events, window, &mut app).expect("the loop should have run");

        assert_eq!(seen.borrow().first(), Some(&Event::Tick { elapsed_ms: 7 }));
    }
}
