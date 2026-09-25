//! Putting the composited frame somewhere a person can see it, and getting
//! keystrokes back.
//!
//! Everything else in this crate was real up to the last step and then stopped:
//! [`Compositor::compose_frame`](crate::Compositor::compose_frame) blends every
//! window and the desktop furniture into a buffer, and
//! [`front_buffer`](crate::Compositor::front_buffer) hands out finished ARGB
//! pixels that nothing looked at. In the other direction,
//! [`handle_input`](crate::Compositor::handle_input) routes keys and clicks
//! faithfully and no key or click ever arrived, because the hosted build has no
//! keyboard driver. Both gaps have the same shape and the same cause — the
//! compositor owned no *device* — so they are closed together, by one trait.
//!
//! ## The trait
//!
//! [`Present`] is deliberately tiny: show a [`Frame`], hand back whatever input
//! has arrived, and say whether the display still exists. That is the whole of
//! what a compositor needs from a screen, and keeping it to three methods is
//! what lets a SlateOS framebuffer, a host window and a deliberate no-op all be
//! the same thing to `Server::run_with`.
//!
//! ## The pointer is a layer, and every presenter draws it
//!
//! A [`Frame`] is the composited picture *and* the pointer to draw over it,
//! kept apart the way a display controller keeps its cursor plane apart from
//! the primary one ([`crate::cursor`] says why). So a pointer that moves over a
//! still desktop is a new frame with an unchanged picture: [`Frame::serial`]
//! says so, and a presenter that keeps its own copy of the picture repaints
//! only the few hundred pixels the pointer left and entered. There is no
//! default for drawing the pointer, on purpose — a default that ignored it
//! would be one every new presenter inherited silently.
//!
//! ## What implements it
//!
//! * [`Headless`] — nothing. The default, and correct in three separate
//!   situations: a test, a remote-only display server whose clients are all
//!   elsewhere, and any platform this crate has not been taught to draw on.
//! * [`host::Window`] on Windows — a real window, drawn with `StretchDIBits`,
//!   with its keyboard and mouse messages translated into
//!   [`crate::InputEvent`]s. This is a **development harness**, and
//!   is described as one in that module: it is how a person can look at the
//!   desktop this compositor draws, on the machine the tree is developed on.
//! * [`drm::DrmScanout`] on SlateOS — the real target. It opens the first
//!   `/dev/dri/cardN` that has a display attached — or the one `--card` named —
//!   and drives **every** monitor plugged into it, each at the mode it is
//!   already running, each with its own pair of dumb buffers and its own page
//!   flip. The frame it is handed is the size of the whole desktop and every
//!   monitor copies out its own rectangle of it, so a second screen costs this
//!   trait nothing: `Self::show` still takes one buffer. This is what closed
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
//! ## Waiting for the user
//!
//! A display is also something the compositor's loop *waits on*: between
//! frames it blocks until a client writes, the user does something, or a
//! deadline comes due, rather than waking on a timer to ask
//! (`known-issues.md` → `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`). Three
//! methods make that possible without the loop knowing what kind of display it
//! has, and all three default to "nothing", which is right for a display with
//! no input:
//!
//! * [`Present::wait_on`] adds the handles input arrives on — evdev's file
//!   descriptors — to the loop's [`WaitSet`].
//! * [`Present::deadline`] says when the display needs a tick even if nothing
//!   arrives: the next hotplug probe, the next key repeat.
//! * [`Present::wait`] does the blocking, for the display whose input is not a
//!   handle at all — the host window, whose input is a Windows message queue.
//!
//! **A display with input it cannot put in the set must give a deadline**, or
//! that input is read only when something else happens to wake the loop.
//!
//! ## What is still missing
//!
//! Nothing in this module — but the SlateOS build only *works* if the process
//! was granted a `ResourceType::InputDevice` capability at spawn, which is the
//! service manager's business rather than the compositor's. Without it every
//! `open` of an input node fails with `EACCES` and
//! [`evdev::EvdevError::Denied`] says so in as many words, because a permission
//! error that looks like a missing file is a day lost to the wrong hypothesis.

use std::io;
use std::time::{Duration, Instant};

use guiremote::WaitSet;
use inputsettings::InputSettings;

use crate::{InputEvent, PointerSprite};

/// The earlier of two optional instants, where `None` is "never".
#[must_use]
pub fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// One frame for a [`Present`] to put on the display: the composited picture
/// and, over it, the pointer.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// `width * height` values in `0xAARRGGBB`, top row first — exactly what
    /// [`Compositor::present_pixels`](crate::Compositor::present_pixels)
    /// returns. A short slice is the caller's bug and an implementation may
    /// draw what it has rather than panicking; the display server must not be
    /// brought down by a bad frame.
    pub pixels: &'a [u32],
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Which picture `pixels` is, when the caller knows.
    ///
    /// Two frames with the same `Some` serial carry the same pixels, so a
    /// presenter holding its own copy of the last picture may skip copying it
    /// again when all that changed is the pointer. `None` promises nothing and
    /// the picture must be taken whole — which is what a caller that is not
    /// keeping count, a test above all, wants by default.
    pub serial: Option<u64>,
    /// The pointer, to be drawn over the picture, or `None` for no pointer.
    pub pointer: Option<&'a PointerSprite>,
}

impl<'a> Frame<'a> {
    /// A picture with no pointer and no serial.
    #[must_use]
    pub const fn new(pixels: &'a [u32], width: u32, height: u32) -> Self {
        Self {
            pixels,
            width,
            height,
            serial: None,
            pointer: None,
        }
    }

    /// The same frame, stamped with which picture it is.
    #[must_use]
    pub const fn with_serial(mut self, serial: u64) -> Self {
        self.serial = Some(serial);
        self
    }

    /// The same frame, with `pointer` over it.
    #[must_use]
    pub const fn with_pointer(mut self, pointer: Option<&'a PointerSprite>) -> Self {
        self.pointer = pointer;
        self
    }
}

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
    /// Put `frame` on the display: its picture, and its pointer over it.
    fn show(&mut self, frame: &Frame<'_>);

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

    /// Add the handles this display's input arrives on to `set`, so that the
    /// loop's wait ends when the user does something.
    ///
    /// The default adds nothing: a display with no input, or one whose input
    /// is not a handle and which therefore overrides [`Self::wait`] instead.
    fn wait_on(&self, _set: &mut WaitSet) {}

    /// When this display next needs the loop to run even if nothing arrives —
    /// a hotplug probe, a key repeat — or `None` if nothing is scheduled.
    ///
    /// An instant already past means "now". The loop wakes at the earliest of
    /// this, its own deadlines and the first handle to become ready, so a
    /// display that forgets to report one does not break the loop, only
    /// delays whatever it was waiting for until something else happens.
    fn deadline(&self) -> Option<Instant> {
        None
    }

    /// Block until something in `set` is ready, this display has input, or
    /// `timeout` passes (`None`: no timeout).
    ///
    /// The default is [`WaitSet::wait`], which is right for every display
    /// whose input is a handle [`Self::wait_on`] can add — or which has none.
    /// Only a display whose input arrives some other way needs its own: the
    /// host window, whose input is a Windows message queue, overrides this to
    /// wake for messages too.
    ///
    /// # Errors
    ///
    /// Whatever the wait reports. The loop treats a failed wait as a reason to
    /// fall back to sleeping, not to stop the desktop.
    fn wait(&mut self, set: &mut WaitSet, timeout: Option<Duration>) -> io::Result<()> {
        set.wait(timeout).map(drop)
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
    fn show(&mut self, _frame: &Frame<'_>) {}
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
    /// The most recent frame's picture, as `(width, height, pixels)`.
    last: Option<(u32, u32, Vec<u32>)>,
    /// The pointer the most recent frame drew over it.
    pointer: Option<PointerSprite>,
    /// The serial the most recent frame carried.
    serial: Option<u64>,
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
    /// Close the display once this many frames have been shown.
    ///
    /// For a test about *when* the loop shows a frame. [`Self::close_after`]
    /// cannot say that: it keeps the loop ticking back to back, so a frame the
    /// loop holds back until the next refresh would never be reached. A
    /// recording closed by this lets the loop wait as it would for a real
    /// display, and ends it as soon as the frame in question arrives.
    pub close_once_shown: Option<u64>,
    /// Close the display at this instant whatever else has happened — the
    /// watchdog that turns a frame never shown into a failed assertion rather
    /// than a hung test.
    pub close_at: Option<Instant>,
    /// Close the display the moment the loop has nothing left to wait for:
    /// its script is spent and it is about to wait with no deadline at all.
    ///
    /// For a test that feeds a burst of input and wants to see everything the
    /// loop does about it, however long a loaded machine takes to do it. No
    /// count of ticks or frames has to be guessed, and no timer has to be
    /// short enough to keep the test quick yet long enough never to fire
    /// early. While this is set, [`Self::close_at`] is still honoured but is
    /// not offered to the loop as a deadline, since a watchdog is not work.
    pub close_when_idle: bool,
    /// Close the display once another thread sets this — the way a test ends
    /// a loop it is not driving. The loop notices the next time it wakes, so
    /// the test must also give it a reason to: hang up a client, say.
    pub stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
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
            pointer: None,
            serial: None,
            shown: 0,
            script: std::collections::VecDeque::new(),
            ticks: 0,
            open: true,
            close_after: None,
            close_once_shown: None,
            close_at: None,
            close_when_idle: false,
            stop: None,
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

    /// The picture of the most recent frame shown, if any — without the
    /// pointer, which is a layer over it: see [`Self::last_pointer`] and
    /// [`Self::seen`].
    #[must_use]
    pub fn last_frame(&self) -> Option<(u32, u32, &[u32])> {
        self.last.as_ref().map(|(w, h, p)| (*w, *h, p.as_slice()))
    }

    /// The pointer the most recent frame drew, if it drew one.
    #[must_use]
    pub const fn last_pointer(&self) -> Option<&PointerSprite> {
        self.pointer.as_ref()
    }

    /// The serial the most recent frame carried: the same one as the frame
    /// before it exactly when the picture did not change.
    #[must_use]
    pub const fn last_serial(&self) -> Option<u64> {
        self.serial
    }

    /// What a person looking at the display sees at `(x, y)`: the picture,
    /// with the pointer laid over it where the pointer is.
    #[must_use]
    pub fn seen(&self, x: u32, y: u32) -> Option<u32> {
        let below = self.pixel(x, y)?;
        let Some(pointer) = self.pointer.as_ref() else {
            return Some(below);
        };
        let mut one = [below];
        let (Ok(px), Ok(py)) = (i32::try_from(x), i32::try_from(y)) else {
            return Some(below);
        };
        let local = PointerSprite {
            image: std::sync::Arc::clone(&pointer.image),
            x: pointer.x.saturating_sub(px),
            y: pointer.y.saturating_sub(py),
        };
        local.blend_over(&mut one, 1, 1);
        Some(one[0])
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
    fn show(&mut self, frame: &Frame<'_>) {
        self.last = Some((frame.width, frame.height, frame.pixels.to_vec()));
        self.pointer = frame.pointer.cloned();
        self.serial = frame.serial;
        self.shown = self.shown.saturating_add(1);
    }

    fn input(&mut self) -> Vec<InputEvent> {
        self.ticks = self.ticks.saturating_add(1);
        self.script.pop_front().unwrap_or_default()
    }

    fn is_open(&self) -> bool {
        self.open
            && self.close_after.is_none_or(|limit| self.ticks < limit)
            && self.close_once_shown.is_none_or(|limit| self.shown < limit)
            && self.close_at.is_none_or(|at| Instant::now() < at)
            && self
                .stop
                .as_ref()
                .is_none_or(|stop| !stop.load(std::sync::atomic::Ordering::Acquire))
    }

    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {
        self.monitors.clone()
    }

    /// A recording is a script, not a device. While it has a batch of input
    /// left, or is counting ticks down to closing, it is ready at once — each
    /// batch is input the loop must go and fetch, and a countdown in ticks
    /// only counts down if the loop ticks. Otherwise it wakes the loop only to
    /// be closed at [`Self::close_at`], and leaves the loop to wait for its
    /// own reasons, exactly as a real display with nobody touching it would.
    fn deadline(&self) -> Option<Instant> {
        if !self.script.is_empty() || self.close_after.is_some() {
            return Some(Instant::now());
        }
        if self.close_when_idle {
            // The watchdog is still checked by `is_open` whenever the loop
            // wakes; offering it as a deadline would stop the loop ever being
            // idle, which is the moment this recording is waiting for.
            return None;
        }
        self.close_at
    }

    /// Closes instead of waiting when [`Self::close_when_idle`] is set and
    /// the loop has no deadline at all; otherwise waits as any display with
    /// no input of its own does.
    fn wait(&mut self, set: &mut WaitSet, timeout: Option<Duration>) -> io::Result<()> {
        if self.close_when_idle && timeout.is_none() {
            self.open = false;
            return Ok(());
        }
        set.wait(timeout).map(drop)
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

    /// Add the handles this source's events arrive on to `set`.
    ///
    /// The [`InputSource`] half of [`Present::wait_on`]. The default adds
    /// nothing, which is right for a source that has no handles — and wrong
    /// for one that has events and no deadline, which the loop would then
    /// read only when something else woke it.
    fn wait_on(&self, _set: &mut WaitSet) {}

    /// When this source next has something to report even if no device says
    /// anything — a held key's next repeat — or `None`.
    ///
    /// The [`InputSource`] half of [`Present::deadline`].
    fn deadline(&self) -> Option<Instant> {
        None
    }
}

/// A screen and an input source, presented as one display.
///
/// The adapter that lets [`Server::run_with`](crate::Server::run_with) stay
/// unchanged now that input has stopped coming from the same device as output.
/// Every method goes to the half that owns it: frames and monitors to the
/// screen, events to the source, and the screen alone decides when the display
/// is gone — a keyboard being unplugged is not a reason to end the session.
///
/// [`Present::show`] is also where [`InputSource::set_bounds`] is kept current.
/// It forwards only on a *change*, so the common case is a comparison of two
/// pairs of integers per frame rather than a call into the pointer.
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
    fn show(&mut self, frame: &Frame<'_>) {
        if self.bounds != (frame.width, frame.height) {
            self.bounds = (frame.width, frame.height);
            self.input.set_bounds(frame.width, frame.height);
        }
        self.screen.show(frame);
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

    /// Both halves: the screen may have input of its own, and the source is
    /// where the keyboard is.
    fn wait_on(&self, set: &mut WaitSet) {
        self.screen.wait_on(set);
        self.input.wait_on(set);
    }

    /// Whichever half needs the loop first: the screen's next hotplug probe or
    /// the source's next key repeat.
    fn deadline(&self) -> Option<Instant> {
        earliest(self.screen.deadline(), self.input.deadline())
    }

    /// The screen's wait, since a screen is what might need a special one; the
    /// source's handles are already in `set`.
    fn wait(&mut self, set: &mut WaitSet, timeout: Option<Duration>) -> io::Result<()> {
        self.screen.wait(set, timeout)
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

    use std::time::{Duration, Instant};

    use inputsettings::InputSettings;

    use super::{Frame, Headless, InputSource, MonitorInfo, Paired, Present, Recording};
    use crate::InputEvent;

    #[test]
    fn a_headless_display_accepts_frames_and_never_closes() {
        let mut headless = Headless;
        headless.show(&Frame::new(&[0xFF00_0000; 4], 2, 2));
        assert!(headless.input().is_empty());
        assert!(headless.is_open(), "a display with no screen never breaks");
    }

    #[test]
    fn a_recording_display_keeps_the_last_frame_and_can_be_asked_for_a_pixel() {
        let mut rec = Recording::new();
        assert_eq!(rec.last_frame(), None, "nothing has been shown");

        // A 3x2 frame, distinct in every cell so a transposed index shows up.
        let frame: Vec<u32> = (0..6).map(|i| 0xFF00_0000 | i).collect();
        rec.show(&Frame::new(&frame, 3, 2));

        assert_eq!(rec.shown(), 1);
        // Row-major, top row first: (2, 1) is the last value.
        assert_eq!(rec.pixel(0, 0), Some(0xFF00_0000));
        assert_eq!(rec.pixel(2, 0), Some(0xFF00_0002));
        assert_eq!(rec.pixel(0, 1), Some(0xFF00_0003));
        assert_eq!(rec.pixel(2, 1), Some(0xFF00_0005));
    }

    /// The recorder keeps the pointer apart from the picture, as a cursor plane
    /// is kept apart from the primary one — and can say what the two look like
    /// together.
    #[test]
    fn a_recording_keeps_the_pointer_as_a_layer_over_the_picture() {
        use crate::{CursorCache, CursorShape, CursorStyle, PointerState};
        let mut cache = CursorCache::new();
        let sprite = cache
            .sprite(&PointerState {
                shape: CursorShape::Arrow,
                x: 10,
                y: 10,
                style: CursorStyle {
                    size_px: 24,
                    fill: 0xFFFF_FFFF,
                    outline: 0xFF00_0000,
                },
            })
            .expect("an arrow");
        let picture = vec![0xFF20_4060u32; 64 * 64];
        let mut rec = Recording::new();
        rec.show(&Frame::new(&picture, 64, 64).with_pointer(Some(&sprite)));

        let (_, _, shown) = rec.last_frame().expect("a frame");
        assert!(
            shown.iter().all(|&p| p == 0xFF20_4060),
            "the pointer was painted into the picture"
        );
        assert_eq!(rec.last_pointer(), Some(&sprite));
        // At the hot spot the arrow's tip is inked, so what is seen there is
        // not the picture; far away from it, it is.
        assert_ne!(
            rec.seen(10, 10),
            Some(0xFF20_4060),
            "the tip is not visible"
        );
        assert_eq!(rec.seen(60, 60), Some(0xFF20_4060));
    }

    #[test]
    fn a_pixel_outside_the_frame_is_none_and_not_a_wrapped_neighbour() {
        // The bug this catches: `y * width + x` with no bounds check reads
        // (3, 0) as (0, 1), which is a real pixel and a wrong answer.
        let mut rec = Recording::new();
        rec.show(&Frame::new(&(0..6).collect::<Vec<u32>>(), 3, 2));
        assert_eq!(rec.pixel(3, 0), None, "one past the right edge");
        assert_eq!(rec.pixel(0, 2), None, "one below the bottom edge");
    }

    #[test]
    fn the_newest_frame_replaces_the_one_before_it() {
        let mut rec = Recording::new();
        rec.show(&Frame::new(&[1, 2, 3, 4], 2, 2));
        rec.show(&Frame::new(&[9, 9, 9, 9], 2, 2));
        assert_eq!(rec.shown(), 2, "both were counted");
        assert_eq!(rec.pixel(0, 0), Some(9), "and the newest is what is there");
    }

    #[test]
    fn a_resized_display_is_reported_at_its_new_size() {
        let mut rec = Recording::new();
        rec.show(&Frame::new(&[0; 4], 2, 2));
        rec.show(&Frame::new(&[0; 6], 3, 2));
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
        /// What it reports as its next deadline.
        due: Option<Instant>,
    }

    impl InputSource for ScriptedSource {
        fn poll(&mut self) -> Vec<InputEvent> {
            self.script.pop_front().unwrap_or_default()
        }

        fn deadline(&self) -> Option<Instant> {
            self.due
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
        pair.show(&Frame::new(&[0xFF00_00AB; 4], 2, 2));
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
        pair.show(&Frame::new(&big, 800, 600));
        pair.show(&Frame::new(&big, 800, 600));
        assert_eq!(
            pair.input.bounds,
            vec![(800, 600)],
            "an unchanged size is two integer comparisons, not a call"
        );

        pair.show(&Frame::new(&small, 640, 480));
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

    // -----------------------------------------------------------------------
    // Waiting
    // -----------------------------------------------------------------------

    #[test]
    fn the_earliest_of_two_deadlines_is_the_sooner_and_none_is_never() {
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        assert_eq!(super::earliest(Some(now), Some(later)), Some(now));
        assert_eq!(super::earliest(Some(later), Some(now)), Some(now));
        assert_eq!(super::earliest(None, Some(later)), Some(later));
        assert_eq!(super::earliest(Some(later), None), Some(later));
        assert_eq!(super::earliest(None, None), None);
    }

    #[test]
    fn a_display_with_no_input_asks_for_nothing_and_waits_on_nothing() {
        let headless = Headless;
        assert_eq!(headless.deadline(), None);
        let mut set = guiremote::WaitSet::new();
        headless.wait_on(&mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn a_pair_wakes_for_whichever_half_needs_it_first() {
        let soon = Instant::now() + Duration::from_millis(30);
        let source = ScriptedSource {
            due: Some(soon),
            ..ScriptedSource::default()
        };
        let mut screen = Recording::new();
        screen.close_at = Some(soon + Duration::from_secs(1));
        let pair = Paired::new(screen, source, 2, 2);
        assert_eq!(
            pair.deadline(),
            Some(soon),
            "the source's key repeat comes first"
        );

        let mut screen = Recording::new();
        screen.close_at = Some(soon);
        let pair = Paired::new(screen, ScriptedSource::default(), 2, 2);
        assert_eq!(
            pair.deadline(),
            Some(soon),
            "and the screen's, when it is the only one"
        );
    }

    #[test]
    fn a_recording_with_input_left_is_ready_at_once_and_an_idle_one_is_not() {
        let mut rec = Recording::new();
        assert_eq!(
            rec.deadline(),
            None,
            "nothing scripted: wait like a real display"
        );

        rec.feed(vec![InputEvent::MouseMove { x: 1, y: 1 }]);
        let before = Instant::now();
        assert!(
            rec.deadline()
                .is_some_and(|at| at >= before && at <= Instant::now())
        );
        rec.input();
        assert_eq!(
            rec.deadline(),
            None,
            "and once the script is read, idle again"
        );

        // A countdown in ticks only counts down if the loop ticks.
        let counting = Recording::closing_after(3);
        assert!(counting.deadline().is_some());
    }

    #[test]
    fn a_recording_can_close_once_it_has_seen_enough_frames_or_at_a_time() {
        let mut rec = Recording::new();
        rec.close_once_shown = Some(1);
        assert!(rec.is_open());
        rec.show(&Frame::new(&[0; 4], 2, 2));
        assert!(!rec.is_open(), "the frame it was waiting for arrived");

        let mut rec = Recording::new();
        let at = Instant::now() + Duration::from_millis(20);
        rec.close_at = Some(at);
        assert!(rec.is_open());
        assert_eq!(rec.deadline(), Some(at), "it wakes the loop to be closed");
        std::thread::sleep(Duration::from_millis(25));
        assert!(!rec.is_open(), "the watchdog fired");
    }

    #[test]
    fn a_recording_closed_when_idle_closes_instead_of_waiting_for_ever() {
        let mut rec = Recording::new();
        rec.close_when_idle = true;
        rec.close_at = Some(Instant::now() + Duration::from_hours(1));
        assert_eq!(
            rec.deadline(),
            None,
            "its watchdog is not work the loop should wake for"
        );
        let mut set = guiremote::WaitSet::new();
        // A bounded wait is still a wait...
        rec.wait(&mut set, Some(Duration::from_millis(1))).unwrap();
        assert!(rec.is_open());
        // ...and an unbounded one is the end of the session.
        rec.wait(&mut set, None).unwrap();
        assert!(!rec.is_open());
    }

    #[test]
    fn a_recording_can_be_stopped_from_another_thread() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let mut rec = Recording::new();
        rec.stop = Some(Arc::clone(&stop));
        assert!(rec.is_open());
        std::thread::spawn(move || stop.store(true, Ordering::Release))
            .join()
            .unwrap();
        assert!(!rec.is_open());
    }
}
