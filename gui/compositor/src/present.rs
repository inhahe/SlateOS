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
//! ## What is still missing
//!
//! Input on SlateOS. [`drm::DrmScanout`] is a screen and only a screen: it
//! inherits the default [`Present::input`], which returns nothing, because the
//! kernel exposes no evdev-style device for the keyboard and mouse yet. A
//! SlateOS desktop therefore draws correctly and cannot be typed at. That is
//! `known-issues.md` → `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`, and it plugs into
//! this same seam — either as a second method on the scanout or as a separate
//! device the binary polls alongside it.

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

pub mod drm;

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

    use super::{Headless, Present, Recording};
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
}
