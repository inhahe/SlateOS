//! Putting the composited frame somewhere a person can see it, and getting
//! keystrokes back.
//!
//! Everything else in this crate was real up to the last step and then stopped:
//! [`Compositor::compose_frame`](crate::Compositor::compose_frame) blends every
//! window, the cursor and the desktop furniture into a buffer, and
//! [`front_buffer`](crate::Compositor::front_buffer) hands out finished ARGB
//! pixels that nothing looked at. In the other direction,
//! [`handle_input`](crate::Compositor::handle_input) routes keys and clicks
//! faithfully and no key or click ever arrived, because the hosted build has no
//! keyboard driver. Both gaps have the same shape and the same cause — the
//! compositor owned no *device* — so they are closed together, by one trait.
//!
//! ## The trait
//!
//! [`Present`] is deliberately tiny: show a rectangle of pixels, hand back
//! whatever input has arrived, and say whether the display still exists. That
//! is the whole of what a compositor needs from a screen, and keeping it to
//! three methods is what lets a SlateOS framebuffer, a host window and a
//! deliberate no-op all be the same thing to [`Server::run_with`].
//!
//! ## What implements it
//!
//! * [`Headless`] — nothing. The default, and correct in three separate
//!   situations: a test, a remote-only display server whose clients are all
//!   elsewhere, and any platform this crate has not been taught to draw on.
//! * [`host::Window`] on Windows — a real window, drawn with `StretchDIBits`,
//!   with its keyboard and mouse messages translated into
//!   [`InputEvent`](crate::InputEvent)s. This is a **development harness**, and
//!   is described as one in that module: it is how a person can look at the
//!   desktop this compositor draws, on the machine the tree is developed on.
//! * [`drm::DrmScanout`] on SlateOS — the real target. It opens the first
//!   `/dev/dri/cardN` that has a display attached — or the one `--card` named —
//!   and drives **every** monitor plugged into it, each at the mode it is
//!   already running, each with its own pair of dumb buffers and its own page
//!   flip. The frame it is handed is the size of the whole desktop and every
//!   monitor copies out its own rectangle of it, so a second screen costs this
//!   trait nothing: [`Self::show`] still takes one buffer. This is what closed
//!   `known-issues.md` → `TD-COMPOSITOR-HAS-NO-SCANOUT` and
//!   `TD-COMPOSITOR-DRIVES-ONE-HEAD`, and neither needed a change to
//!   [`Server::run_with`](crate::Server::run_with) — which is the claim this
//!   trait was designed to make good on.
//!
//! ## The screen and the keyboard are different devices
//!
//! [`drm::DrmScanout`] is a screen and only a screen: it inherits the default
//! [`Present::input`], which returns nothing, because a graphics card is not
//! where keystrokes come from. Those arrive from `/dev/input/eventN`, which is
//! [`evdev::EvdevInput`] — and that is an [`InputSource`], not a [`Present`],
//! precisely because it has no frame to show.
//!
//! [`Paired`] is what makes one out of two. It holds a screen and an input
//! source, forwards each method to whichever half owns it, and is itself a
//! [`Present`], so [`Server::run_with`](crate::Server::run_with) never learns
//! that its display grew a keyboard. This is what closed `known-issues.md` →
//! `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`.
//!
//! ## What is still missing
//!
//! Nothing in this module — but the SlateOS build only *works* if the process
//! was granted a `ResourceType::InputDevice` capability at spawn, which is the
//! service manager's business rather than the compositor's. Without it every
//! `open` of an input node fails with `EACCES` and
//! [`evdev::EvdevError::Denied`] says so in as many words, because a permission
//! error that looks like a missing file is a day lost to the wrong hypothesis.

use inputsettings::InputSettings;

use crate::InputEvent;

/// One monitor a [`Present`] is driving.
///
/// The scanout's side of the desktop arrangement, reported so that
/// [`Server::run_with`](crate::Server::run_with) can keep the compositor's side
/// in step with it when a monitor is plugged in or unplugged. It is deliberately
/// four numbers and not a display: what a monitor is *called*, what it is scaled
/// by and whether it is primary are the compositor's business, and a scanout
/// that offered opinions on them would be a second place those facts live.
///
/// **There is no position in it, on purpose.** The two layouts agree by
/// construction rather than by protocol — both lay monitors out left-to-right in
/// enumeration order and neither re-flows the survivors when one leaves
/// (design-decisions.md §515, §516) — so a scanout sending coordinates would be
/// sending the compositor a number it is about to derive identically anyway,
/// and the first time the two disagreed the bug would be silent on both sides.
/// Withholding it means there is one rule, applied twice, instead of two rules
/// that have to be kept in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Stable identity of this monitor, unique among the ones a given display is
    /// driving and unchanged for as long as it stays plugged in.
    ///
    /// On DRM this is the **connector id** — the socket on the card rather than
    /// a position in a list — for the reason §515 gives: a position means two
    /// different things once a head can die, and an id means one.
    pub id: u32,
    /// Width in pixels of the mode it is running.
    pub width: u32,
    /// Height in pixels of the mode it is running.
    pub height: u32,
    /// Refresh rate in Hz, or a nominal 60 where the display cannot say.
    pub refresh_hz: u32,
}

/// Somewhere to put a frame, and somewhere input comes from.
///
/// Implementors are expected to be cheap to call at the display's refresh rate:
/// [`Self::show`] runs once per composited frame and [`Self::input`] once per
/// tick.
pub trait Present {
    /// Put `pixels` on the display.
    ///
    /// `pixels` is `width * height` values in `0xAARRGGBB`, top row first —
    /// exactly what [`Compositor::front_buffer`](crate::Compositor::front_buffer)
    /// returns. A short slice is the caller's bug and an implementation may
    /// draw what it has rather than panicking; the display server must not be
    /// brought down by a bad frame.
    fn show(&mut self, pixels: &[u32], width: u32, height: u32);

    /// Whatever the user has done since the last call.
    ///
    /// Returns owned events rather than borrowing an internal queue, because a
    /// caller feeding them to
    /// [`Compositor::handle_input`](crate::Compositor::handle_input) holds the
    /// compositor mutably and could not hold a borrow of this at the same time.
    /// The vector is empty on an idle desktop, which is the common case, and an
    /// empty `Vec` allocates nothing.
    fn input(&mut self) -> Vec<InputEvent> {
        Vec::new()
    }

    /// Whether the display still exists.
    ///
    /// A host window the user closed returns `false`, and
    /// [`Server::run_with`](crate::Server::run_with) stops. A framebuffer
    /// returns `true` until the machine does.
    fn is_open(&self) -> bool {
        true
    }

    /// The monitors this display is driving right now, or `None` if it is not
    /// the sort of display that has monitors.
    ///
    /// This is the *detect* half of monitor hotplug. The compositor keeps its
    /// own arrangement — that is what places windows and answers "which screen
    /// is this on?" — and it has no way to learn that a monitor arrived or left,
    /// because the only thing holding a connector is the scanout. Asking here
    /// once a tick, and reconciling the two by id, is what makes plugging a
    /// second screen in do something.
    ///
    /// **Polled rather than pushed**, and returning the whole set rather than a
    /// list of changes, because that makes the answer idempotent: asking twice
    /// gives the same reply, a reconciliation that was skipped or that failed is
    /// simply retried on the next tick, and there is no queue of changes to get
    /// out of step with the thing it describes. The cost is a short `Vec` per
    /// tick, which an implementation is free to build from a cache it refreshes
    /// far less often than it is asked.
    ///
    /// `None` and `Some(vec![])` are different answers and neither means "no
    /// monitors". `None` is *no opinion* — a headless server, a host window,
    /// anything with no connectors to enumerate — and the caller must leave the
    /// arrangement alone. An empty list means the display has lost every monitor
    /// it had, which is not an arrangement any compositor can adopt; such a
    /// display is expected to answer `false` to [`Self::is_open`] and be shut
    /// down rather than reconciled to nothing.
    ///
    /// Takes `&mut self` because the honest implementation of it re-probes
    /// hardware.
    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {
        None
    }

    /// Adopt the user's input preferences, which have just changed.
    ///
    /// Pointer speed, acceleration, button mapping, scroll direction and the
    /// key-repeat rate are all applied where raw device deltas arrive — which
    /// is here and not in the compositor, because a *relative* mouse delta is
    /// only a pointer position once someone has integrated it, and the thing
    /// doing the integrating is the input source. The compositor reads
    /// `input.yaml` and knows what it says; it has no device to say it to.
    ///
    /// Called by [`Server::run_with`](crate::Server::run_with) when
    /// [`Compositor::input_settings`](crate::Compositor::input_settings) starts
    /// answering something other than what was last passed here. That is the
    /// same polled, idempotent shape as [`Self::monitors`], for the same
    /// reason: a push would need a queue, and a queue is a thing that can get
    /// out of step with what it describes.
    ///
    /// The default body ignores it, like [`Self::monitors`]'s: a headless
    /// server, a recording and a host window have no pointer whose speed could
    /// change. Only the implementor that owns a device needs to care.
    fn reload_input(&mut self, _settings: &InputSettings) {}
}

/// A display server with no display.
///
/// Not a stub standing in for something unwritten — a real and correct choice.
/// A compositor serving only remote clients has no local screen to draw on, and
/// every test in this tree wants exactly this: the full pipeline, up to and
/// including the composited buffer, with nothing that needs a window manager or
/// a graphics device to be present on the build machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct Headless;

impl Present for Headless {
    fn show(&mut self, _pixels: &[u32], _width: u32, _height: u32) {}
}

/// A [`Present`] that keeps the last frame, so a test can look at it.
///
/// The thing [`Headless`] cannot do: assert that the compositor drew what it
/// was asked to. `front_buffer()` is reachable directly from a test that owns
/// the `Compositor`, but not from one driving [`Server::run_with`](crate::Server::run_with),
/// which owns it for the duration — and "what reached the screen" is a
/// different claim from "what was in the buffer at some point", which is
/// precisely the distinction this module exists to make.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    /// The most recent frame, as `(width, height, pixels)`.
    last: Option<(u32, u32, Vec<u32>)>,
    /// How many frames have been shown.
    shown: u64,
    /// Input to hand back, one batch per call to [`Present::input`].
    pub script: std::collections::VecDeque<Vec<InputEvent>>,
    /// How many times [`Present::input`] has been called.
    ticks: u64,
    /// Set to `false` to make the display go away, as a closed window does.
    pub open: bool,
    /// Close the display after this many ticks, as if the user had shut the
    /// window.
    ///
    /// This is what makes [`Server::run_with`](crate::Server::run_with)
    /// testable at all: it is a loop that runs until the display goes away, so
    /// a test driving the *real* loop — rather than a hand-rolled imitation of
    /// it, which is exactly the sort of copy that stops resembling the original
    /// — needs some way to end it. Counted in ticks rather than frames on
    /// purpose: a frame is only composed when there is something to draw, so a
    /// count of frames would never be reached on an idle desktop and the test
    /// would hang instead of failing.
    pub close_after: Option<u64>,
    /// What [`Present::monitors`] answers, if this recorder is standing in for a
    /// display that has monitors at all.
    ///
    /// `None` — the default — is a recorder with no opinion, which is what every
    /// test that predates hotplug wants: it leaves the compositor's display
    /// arrangement exactly as the test built it. Set it to make a monitor arrive
    /// or leave in the middle of a real
    /// [`Server::run_with`](crate::Server::run_with) loop, which is otherwise
    /// only reachable with a graphics card.
    pub monitors: Option<Vec<MonitorInfo>>,
}

impl Recording {
    /// A recorder with nothing shown yet and an open display.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last: None,
            shown: 0,
            script: std::collections::VecDeque::new(),
            ticks: 0,
            open: true,
            close_after: None,
            monitors: None,
        }
    }

    /// A recorder that closes itself after `ticks` calls to [`Present::input`].
    #[must_use]
    pub fn closing_after(ticks: u64) -> Self {
        Self {
            close_after: Some(ticks),
            ..Self::new()
        }
    }

    /// The most recent frame shown, if any.
    #[must_use]
    pub fn last_frame(&self) -> Option<(u32, u32, &[u32])> {
        self.last.as_ref().map(|(w, h, p)| (*w, *h, p.as_slice()))
    }

    /// How many frames have reached the display.
    #[must_use]
    pub const fn shown(&self) -> u64 {
        self.shown
    }

    /// How many times the display has been asked for input.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The colour at a point of the last frame, if it is inside it.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        let (w, h, pixels) = self.last.as_ref()?;
        if x >= *w || y >= *h {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(*w).ok()?)?;
        pixels
            .get(index.checked_add(usize::try_from(x).ok()?)?)
            .copied()
    }

    /// Queue a batch of input to be returned by the next [`Present::input`].
    pub fn feed(&mut self, batch: Vec<InputEvent>) {
        self.script.push_back(batch);
    }
}

impl Present for Recording {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32) {
        self.last = Some((width, height, pixels.to_vec()));
        self.shown = self.shown.saturating_add(1);
    }

    fn input(&mut self) -> Vec<InputEvent> {
        self.ticks = self.ticks.saturating_add(1);
        self.script.pop_front().unwrap_or_default()
    }

    fn is_open(&self) -> bool {
        self.open && self.close_after.is_none_or(|limit| self.ticks < limit)
    }

    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {
        self.monitors.clone()
    }
}

/// Somewhere input comes from that is not a screen.
///
/// The other half of [`Present`], split off because on the real target they are
/// two devices and not one: a graphics card produces no keystrokes and a
/// keyboard has no frame to show. Implementing [`Present`] for a keyboard would
/// mean writing a [`Present::show`] that throws its argument away, which is a
/// lie the type system would then let anyone tell.
///
/// Pair one of these with a screen using [`Paired`] to get something
/// [`Server::run_with`](crate::Server::run_with) can drive.
pub trait InputSource {
    /// Whatever the user has done since the last call.
    ///
    /// Same contract as [`Present::input`]: owned events, empty on an idle
    /// desktop, called once per tick.
    fn poll(&mut self) -> Vec<InputEvent>;

    /// Tell the source how big the desktop is.
    ///
    /// A pointer needs this and a keyboard does not, hence the default. An
    /// evdev mouse reports *relative* motion, so the only thing that knows
    /// where the pointer ended up is whoever integrated those deltas — and it
    /// cannot clamp the result to the screen without being told what the screen
    /// is. Called whenever the composited frame changes size, which covers
    /// monitor hotplug; the initial size has to come from construction, because
    /// [`Server::run_with`](crate::Server::run_with) polls input *before* it
    /// shows the first frame.
    fn set_bounds(&mut self, _width: u32, _height: u32) {}

    /// Adopt the user's input preferences, which have just changed.
    ///
    /// The [`InputSource`] half of [`Present::reload_input`], and the one that
    /// actually does the work: a source that integrates relative deltas into a
    /// pointer position is the only thing in the system that can apply a
    /// pointer speed, and the only thing that can decide a key has repeated is
    /// the thing holding the key-down timestamp. The default ignores the
    /// settings for the same reason [`Self::set_bounds`]'s does — a source with
    /// no pointer and no repeat clock has nothing to change.
    fn reload_input(&mut self, _settings: &InputSettings) {}
}

/// A screen and an input source, presented as one display.
///
/// The adapter that lets [`Server::run_with`](crate::Server::run_with) stay
/// unchanged now that input has stopped coming from the same device as output.
/// Every method goes to the half that owns it: frames and monitors to the
/// screen, events to the source, and the screen alone decides when the display
/// is gone — a keyboard being unplugged is not a reason to end the session.
///
/// [`Self::show`] is also where [`InputSource::set_bounds`] is kept current. It
/// forwards only on a *change*, so the common case is a comparison of two pairs
/// of integers per frame rather than a call into the pointer.
#[derive(Clone, Copy, Debug)]
pub struct Paired<S, I> {
    /// The half that draws.
    screen: S,
    /// The half that listens.
    input: I,
    /// The last size passed to [`Present::show`], so a resize can be spotted.
    bounds: (u32, u32),
}

impl<S: Present, I: InputSource> Paired<S, I> {
    /// Pair a screen with an input source.
    ///
    /// `width` and `height` are the desktop's size at start-up, which the
    /// source is told immediately rather than on the first frame: `run_with`
    /// polls input before it shows anything, so a source that waited for
    /// [`Present::show`] would spend its first tick not knowing where the edges
    /// of the screen are.
    pub fn new(screen: S, mut input: I, width: u32, height: u32) -> Self {
        input.set_bounds(width, height);
        Self {
            screen,
            input,
            bounds: (width, height),
        }
    }

    /// The screen half, for a caller that needs it back.
    pub const fn screen(&self) -> &S {
        &self.screen
    }

    /// The input half, mutably — for reloading settings while running.
    pub const fn input_mut(&mut self) -> &mut I {
        &mut self.input
    }
}

impl<S: Present, I: InputSource> Present for Paired<S, I> {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32) {
        if self.bounds != (width, height) {
            self.bounds = (width, height);
            self.input.set_bounds(width, height);
        }
        self.screen.show(pixels, width, height);
    }

    fn input(&mut self) -> Vec<InputEvent> {
        self.input.poll()
    }

    fn is_open(&self) -> bool {
        self.screen.is_open()
    }

    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {
        self.screen.monitors()
    }

    fn reload_input(&mut self, settings: &InputSettings) {
        self.input.reload_input(settings);
    }
}

pub mod drm;

pub mod evdev;

#[cfg(windows)]
pub mod host;

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

    use inputsettings::InputSettings;

    use super::{Headless, InputSource, MonitorInfo, Paired, Present, Recording};
    use crate::InputEvent;

    #[test]
    fn a_headless_display_accepts_frames_and_never_closes() {
        let mut headless = Headless;
        headless.show(&[0xFF00_0000; 4], 2, 2);
        assert!(headless.input().is_empty());
        assert!(headless.is_open(), "a display with no screen never breaks");
    }

    #[test]
    fn a_recording_display_keeps_the_last_frame_and_can_be_asked_for_a_pixel() {
        let mut rec = Recording::new();
        assert_eq!(rec.last_frame(), None, "nothing has been shown");

        // A 3x2 frame, distinct in every cell so a transposed index shows up.
        let frame: Vec<u32> = (0..6).map(|i| 0xFF00_0000 | i).collect();
        rec.show(&frame, 3, 2);

        assert_eq!(rec.shown(), 1);
        // Row-major, top row first: (2, 1) is the last value.
        assert_eq!(rec.pixel(0, 0), Some(0xFF00_0000));
        assert_eq!(rec.pixel(2, 0), Some(0xFF00_0002));
        assert_eq!(rec.pixel(0, 1), Some(0xFF00_0003));
        assert_eq!(rec.pixel(2, 1), Some(0xFF00_0005));
    }

    #[test]
    fn a_pixel_outside_the_frame_is_none_and_not_a_wrapped_neighbour() {
        // The bug this catches: `y * width + x` with no bounds check reads
        // (3, 0) as (0, 1), which is a real pixel and a wrong answer.
        let mut rec = Recording::new();
        rec.show(&(0..6).collect::<Vec<u32>>(), 3, 2);
        assert_eq!(rec.pixel(3, 0), None, "one past the right edge");
        assert_eq!(rec.pixel(0, 2), None, "one below the bottom edge");
    }

    #[test]
    fn the_newest_frame_replaces_the_one_before_it() {
        let mut rec = Recording::new();
        rec.show(&[1, 2, 3, 4], 2, 2);
        rec.show(&[9, 9, 9, 9], 2, 2);
        assert_eq!(rec.shown(), 2, "both were counted");
        assert_eq!(rec.pixel(0, 0), Some(9), "and the newest is what is there");
    }

    #[test]
    fn a_resized_display_is_reported_at_its_new_size() {
        let mut rec = Recording::new();
        rec.show(&[0; 4], 2, 2);
        rec.show(&[0; 6], 3, 2);
        let (w, h, pixels) = rec.last_frame().unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(pixels.len(), 6);
    }

    #[test]
    fn scripted_input_comes_back_one_batch_per_call() {
        // One batch per call, not all of it at once: a tick delivers what has
        // arrived since the last tick, and a test that got everything on the
        // first call could not check that the compositor handles a sequence.
        let mut rec = Recording::new();
        rec.feed(vec![InputEvent::MouseMove { x: 1, y: 2 }]);
        rec.feed(vec![InputEvent::KeyDown {
            scancode: 0x1E,
            character: Some('a'),
        }]);

        assert!(matches!(
            rec.input().as_slice(),
            [InputEvent::MouseMove { x: 1, y: 2 }]
        ));
        assert!(matches!(
            rec.input().as_slice(),
            [InputEvent::KeyDown { scancode: 0x1E, .. }]
        ));
        assert!(
            rec.input().is_empty(),
            "and then nothing, rather than a repeat"
        );
    }

    #[test]
    fn a_display_told_to_close_after_n_ticks_stays_open_for_exactly_n() {
        // Off by one here is the difference between a test that drives the real
        // loop and a test that hangs, so it is worth pinning: the display is
        // open for the tick that takes it to the limit and shut afterwards.
        let mut rec = Recording::closing_after(2);
        assert!(rec.is_open(), "before the first tick");
        let _ = rec.input();
        assert!(rec.is_open(), "one tick of two");
        let _ = rec.input();
        assert!(!rec.is_open(), "two of two, and that is the last");
        assert_eq!(rec.ticks(), 2);
    }

    #[test]
    fn a_recorder_with_no_limit_stays_open_however_long_it_runs() {
        let mut rec = Recording::new();
        for _ in 0..100 {
            let _ = rec.input();
        }
        assert!(rec.is_open());
        assert_eq!(rec.ticks(), 100);
    }

    #[test]
    fn a_display_can_be_made_to_go_away() {
        let mut rec = Recording::new();
        assert!(rec.is_open());
        rec.open = false;
        assert!(!rec.is_open(), "which is what a closed window looks like");
    }

    // -----------------------------------------------------------------------
    // Pairing a screen with an input source
    // -----------------------------------------------------------------------

    /// An input source that records what it was told and hands back a script.
    #[derive(Debug, Default)]
    struct ScriptedSource {
        /// Batches to return, one per [`InputSource::poll`].
        script: std::collections::VecDeque<Vec<InputEvent>>,
        /// Every size this source was told about, in order.
        bounds: Vec<(u32, u32)>,
        /// Every settings it was told about, in order.
        reloads: Vec<InputSettings>,
    }

    impl InputSource for ScriptedSource {
        fn poll(&mut self) -> Vec<InputEvent> {
            self.script.pop_front().unwrap_or_default()
        }

        fn set_bounds(&mut self, width: u32, height: u32) {
            self.bounds.push((width, height));
        }

        fn reload_input(&mut self, settings: &InputSettings) {
            self.reloads.push(settings.clone());
        }
    }

    #[test]
    fn a_pair_sends_frames_to_the_screen_and_takes_events_from_the_source() {
        let mut source = ScriptedSource::default();
        source
            .script
            .push_back(vec![InputEvent::MouseMove { x: 7, y: 9 }]);
        let mut pair = Paired::new(Recording::new(), source, 2, 2);

        assert!(matches!(
            pair.input().as_slice(),
            [InputEvent::MouseMove { x: 7, y: 9 }]
        ));
        pair.show(&[0xFF00_00AB; 4], 2, 2);
        assert_eq!(pair.screen().pixel(0, 0), Some(0xFF00_00AB));
        // The screen was never asked for input and the source was never asked
        // to draw: each half only does the thing it is.
        assert_eq!(pair.screen().ticks(), 0);
    }

    #[test]
    fn a_source_learns_the_desktop_size_before_the_first_frame_is_shown() {
        // `Server::run_with` polls input *before* it shows anything, so a
        // source that waited for `show` would spend its first tick not knowing
        // where the edges of the screen are — and a pointer would clamp to a
        // desktop of nothing.
        let pair = Paired::new(Recording::new(), ScriptedSource::default(), 1920, 1080);
        assert_eq!(pair.input.bounds, vec![(1920, 1080)]);
    }

    #[test]
    fn a_resized_desktop_is_passed_on_but_an_unchanged_one_is_not() {
        let mut pair = Paired::new(Recording::new(), ScriptedSource::default(), 800, 600);
        // Real frames rather than short ones, on the heap: a frame whose pixel
        // count did not match its stated size would be testing against a
        // display that could never happen.
        let big = vec![0u32; 800 * 600];
        let small = vec![0u32; 640 * 480];
        pair.show(&big, 800, 600);
        pair.show(&big, 800, 600);
        assert_eq!(
            pair.input.bounds,
            vec![(800, 600)],
            "an unchanged size is two integer comparisons, not a call"
        );

        pair.show(&small, 640, 480);
        assert_eq!(pair.input.bounds, vec![(800, 600), (640, 480)]);
    }

    #[test]
    fn the_screen_alone_decides_when_the_session_ends() {
        let mut pair = Paired::new(Recording::new(), ScriptedSource::default(), 2, 2);
        assert!(pair.is_open());
        pair.screen.open = false;
        // A keyboard being unplugged is not a reason to end the session, so
        // there is no way for the source to answer this at all.
        assert!(!pair.is_open());
    }

    #[test]
    fn the_monitors_are_the_screens_and_the_pairing_does_not_invent_any() {
        let mut bare = Paired::new(Recording::new(), ScriptedSource::default(), 2, 2);
        assert_eq!(bare.monitors(), None, "a recorder with no opinion");

        let heads = vec![MonitorInfo {
            id: 42,
            width: 800,
            height: 600,
            refresh_hz: 60,
        }];
        let mut screen = Recording::new();
        screen.monitors = Some(heads.clone());
        let mut pair = Paired::new(screen, ScriptedSource::default(), 800, 600);
        assert_eq!(pair.monitors(), Some(heads));
    }

    #[test]
    fn the_input_half_can_be_reached_again_to_reload_its_settings() {
        let mut pair = Paired::new(Recording::new(), ScriptedSource::default(), 2, 2);
        pair.input_mut()
            .script
            .push_back(vec![InputEvent::KeyUp { scancode: 0x1E }]);
        assert!(matches!(
            pair.input().as_slice(),
            [InputEvent::KeyUp { scancode: 0x1E }]
        ));
    }

    #[test]
    fn a_pair_hands_the_users_input_settings_to_the_source_and_not_to_the_screen() {
        // The whole reason `Present::reload_input` exists: a pointer speed is
        // applied where the raw deltas are integrated, which is the source, and
        // a screen has no pointer at all. Forwarding to the wrong half would
        // compile and do nothing — the default body ignores its argument — so
        // the check is that the source *did* hear it.
        let mut settings = InputSettings::default();
        settings.mouse.speed = 7;
        let mut pair = Paired::new(Recording::new(), ScriptedSource::default(), 2, 2);

        pair.reload_input(&settings);

        assert_eq!(
            pair.input_mut().reloads.len(),
            1,
            "the source was told, exactly once"
        );
        assert_eq!(pair.input_mut().reloads[0].mouse.speed, 7);
    }

    #[test]
    fn a_display_with_no_pointer_ignores_the_input_settings_rather_than_refusing_them() {
        // The default body, exercised on purpose. A headless server and a
        // recording have nothing whose speed could change, and the alternative
        // to a no-op default is every implementor writing one — which is how a
        // trait grows a method that half its implementors get wrong.
        let mut headless = Headless;
        headless.reload_input(&InputSettings::default());
        let mut rec = Recording::new();
        rec.reload_input(&InputSettings::default());
        assert!(headless.is_open() && rec.is_open(), "and nothing broke");
    }
}
