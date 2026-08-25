//! Local input on SlateOS: reading the keyboard and the mouse.
//!
//! This is the other half of [`drm`](super::drm). That module closed
//! `known-issues.md` → `TD-COMPOSITOR-HAS-NO-SCANOUT` and left the desktop in a
//! state where it drew correctly and could not be typed at; this one closes
//! `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`, which was the same gap in the other
//! direction. [`Compositor::handle_input`](crate::Compositor::handle_input) and
//! its routing were complete and correct the whole time and simply had no
//! source on this platform.
//!
//! ## What it reads
//!
//! Linux `evdev` character devices: `/dev/input/event0` is the keyboard and
//! `/dev/input/event1` the mouse on a SlateOS machine today, but **no index is
//! hardcoded and no device is assumed to be one thing or the other.** Every
//! node that opens is read, and each *record* is routed by its own type —
//! `EV_KEY` below `BTN_MISC` is a keyboard key, `EV_KEY` at or above it is a
//! mouse button, `EV_REL` is pointer motion or scroll. That is what the evdev
//! ABI actually guarantees, and it means a keyboard with a trackpoint, a second
//! mouse, or a machine whose nodes are numbered differently all work without a
//! change here.
//!
//! ## The capability, and the failure that looks like something else
//!
//! **`open` returns `EACCES` unless the process holds a
//! `ResourceType::InputDevice` capability with `Rights::READ`.** That is
//! deliberate on the kernel's side — without it every keystroke on the machine,
//! passwords included, would be readable by anything that can name a path — and
//! it is obtained at spawn (`SpawnOptions.capabilities`) or inherited from a
//! parent that holds it. It is **not** obtainable from inside this process, so
//! an `EACCES` here is not a bug in this module and cannot be fixed by changing
//! it: the compositor's ancestor (init / the service manager) must be granting
//! it. See `requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`
//! and the request it filed onward,
//! `requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`.
//! [`EvdevError::Denied`] exists to say exactly that, in those words, rather
//! than letting a permission error be reported as a missing file.
//!
//! ## Three things the kernel does not do, which are therefore done here
//!
//! 1. **Key repeat.** The kernel has no repeat timer and says so — `EVIOCGREP`
//!    returns `ENOSYS` and autorepeat (`EV_KEY` value 2) is never generated.
//!    Repeat is synthesised from key-down/up timing, which is what a Wayland
//!    compositor does anyway, and takes its delay and interval from the user's
//!    own `input.yaml` ([`inputsettings::KeyboardRepeatConfig`]) — settings that
//!    the Settings panel has been writing and nothing has been reading.
//! 2. **Absolute pointer position.** A mouse reports *deltas*; a compositor
//!    needs a point. [`Pointer`] integrates them, applies the user's pointer
//!    speed and acceleration profile, and clamps the result to the desktop —
//!    keeping the sub-pixel remainder, without which a slow movement at a low
//!    speed setting would round to zero every packet and the pointer would
//!    never move at all.
//! 3. **Re-synchronisation after a drop.** Each open fd has its own cursor into
//!    a bounded ring, and a reader that falls far enough behind to be lapped
//!    gets one `EV_SYN`/`SYN_DROPPED` and then resumes from the current head.
//!    The stream carries only *transitions*, so after a drop the idea of which
//!    keys are held is unreliable: [`EvdevInput`] re-queries `EVIOCGKEY` and
//!    reconciles both ways, releasing keys that are no longer down and pressing
//!    ones that are. Without that a Shift held across a drop would stay stuck
//!    down for ever, and every subsequent letter would be a capital.
//!
//! ## Why this is split in three, again
//!
//! Same reason [`drm`](super::drm) is. Every bug this module can have is a
//! protocol or policy bug — a field at the wrong offset, a keycode mapped to
//! the wrong physical key, a scroll direction inverted, a repeat that never
//! stops — and none of them need a keyboard to find, but all of them are
//! invisible if the whole module is behind `#[cfg(target_os = "linux")]`,
//! because the machine this tree is compiled and tested on is not Linux. So:
//! [`uapi`] is the wire format and the keycode table, compiled and tested
//! everywhere; [`sys`] is the four syscalls that genuinely cannot run off the
//! target, behind a trait; and this file holds all of the decisions and is
//! driven in tests by a fake device that scripts a real byte stream.

pub mod sys;
pub mod uapi;

use std::time::{Duration, Instant};

use inputsettings::{AccelProfile, ButtonMapping, InputSettings, MouseConfig};

use super::InputSource;
use crate::keymap::key_for_scancode;
use crate::{InputEvent, MouseButton};
use guitk::event::Key;
use sys::{DeviceSource, EACCES, EAGAIN, EINTR, EventSys, MAX_DEVICES};
use uapi::{
    BTN_EXTRA, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE, EV_KEY, EV_MSC, EV_REL, EV_SYN,
    EVENT_SIZE, KEY_VALUE_DOWN, KEY_VALUE_REPEAT, KEY_VALUE_UP, MSC_SCAN, REL_HWHEEL, REL_WHEEL,
    REL_X, REL_Y, Record, SYN_DROPPED, SYN_REPORT,
};

/// Bytes read from one device in one `read`. Thirty-two records.
///
/// Sized so that an ordinary frame's worth of input — a keystroke is three
/// records, a mouse packet four — comes back in a single syscall, without
/// putting a kilobyte-scale buffer on the stack of the compositing loop.
const READ_CHUNK: usize = 32 * EVENT_SIZE;

/// Most `read`s issued to one device in one tick.
///
/// A bound rather than "until `EAGAIN`" because a device that produces events
/// faster than they are consumed would otherwise never let the loop out, and
/// the desktop would stop drawing while remaining perfectly responsive to
/// input, which is the worst of both. Eight chunks is 256 records per device
/// per frame — far more than a person can generate — and anything beyond it is
/// left for the next tick rather than dropped.
const MAX_READS_PER_TICK: usize = 8;

// The bound has to be small to be a bound at all, and no test can say so:
// `a_burst_larger_than_one_tick_can_take_is_left_for_the_next_one` feeds
// `MAX_READS_PER_TICK + 2` chunks and expects `MAX_READS_PER_TICK * 8` events,
// so it scales with whatever this constant says and stays green at any value.
// It proves the loop is bounded; it cannot prove the bound is a useful size.
//
// Writing a literal into the test instead would be the same number in two
// places, which is the duplicate-range shape §524 records — so the ceiling is
// asserted here, where raising it fails the build rather than a test.
const _: () = assert!(
    MAX_READS_PER_TICK <= 16,
    "more than 16 chunks (512 records) per device per frame stops bounding the \
     compositing loop, which is the only thing this constant is for"
);

/// Bytes of key bitmap asked of `EVIOCGKEY`.
///
/// `KEY_MAX` is 0x2FF in the Linux ABI, so a full bitmap is 96 bytes. Asking
/// for the whole thing rather than only the range we can translate, because the
/// kernel returns however much it has and a short answer is information (this
/// device has fewer keys) rather than an error.
const KEY_BITMAP_BYTES: usize = 96;

/// Longest device name read from `EVIOCGNAME`, for the startup diagnostic.
const NAME_BYTES: usize = 128;

/// The most a pointer movement may be multiplied by, however the acceleration
/// curve is configured.
///
/// A guard rail rather than a tuning knob: `accel_gain` is user-editable and
/// nothing stops someone typing 10, at which point a flick of the wrist would
/// throw the pointer across a 4K desktop and out the other side. The clamp
/// keeps a badly-chosen setting merely fast instead of unusable.
const MAX_ACCELERATION: f32 = 4.0;

/// Why local input could not be set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvdevError {
    /// Every device node refused to open with `EACCES`.
    ///
    /// Its own variant, and not merely "no devices", because the two need
    /// completely different responses and only one of them is actionable from
    /// this side of the tree. See the module docs: the fix is a capability
    /// grant in the compositor's *ancestor*, and no amount of retrying,
    /// reordering or path-guessing here will produce one.
    Denied,
    /// No device node could be opened, for reasons other than permission —
    /// a machine with no input devices, or a kernel that exposes none.
    NoDevices,
}

impl core::fmt::Display for EvdevError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Denied => f.write_str(
                "permission denied opening /dev/input/event*; the compositor was \
                 not granted an InputDevice capability at spawn and cannot grant \
                 itself one",
            ),
            Self::NoDevices => f.write_str("no /dev/input/event* device could be opened"),
        }
    }
}

// ---------------------------------------------------------------------------
// The pointer
// ---------------------------------------------------------------------------

/// Where the mouse is, derived from the deltas it reports.
///
/// Held in `f32` rather than `i32` on purpose. The user's pointer speed can be
/// as low as 0.25×, at which a one-count movement is a quarter of a pixel; an
/// integer position would round that to zero every packet and the pointer would
/// be immovable at the setting a person with a very sensitive mouse would
/// choose. Keeping the fraction means four counts move it one pixel, which is
/// what the setting means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// Horizontal position, in desktop pixels.
    x: f32,
    /// Vertical position, in desktop pixels.
    y: f32,
    /// Desktop width in pixels. The pointer is clamped to `0..width`.
    width: u32,
    /// Desktop height in pixels. The pointer is clamped to `0..height`.
    height: u32,
}

impl Pointer {
    /// A pointer at the centre of a `width` by `height` desktop.
    ///
    /// The centre rather than the origin because the origin is under the
    /// top-left of whatever is there — a menu button, a window's close box —
    /// and a desktop that comes up with the pointer already hovering a control
    /// looks like it has been clicked on.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let mut pointer = Self {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        pointer.x = f64::from(width).mul_add(0.5, 0.0) as f32;
        pointer.y = f64::from(height).mul_add(0.5, 0.0) as f32;
        pointer.clamp();
        pointer
    }

    /// The position, as the compositor wants it.
    #[must_use]
    pub fn position(&self) -> (i32, i32) {
        (self.x as i32, self.y as i32)
    }

    /// Resize the desktop the pointer lives on, keeping it inside.
    ///
    /// Called whenever a frame of a different size is shown, which is how a
    /// monitor being unplugged reaches this module: the desktop shrinks, and a
    /// pointer left at its old position would be off-screen and unreachable,
    /// because every subsequent movement would be clamped back to a coordinate
    /// that is not displayed.
    pub fn set_bounds(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.clamp();
    }

    /// Move by a raw device delta, applying the user's speed and acceleration.
    fn nudge(&mut self, dx: i32, dy: i32, config: &MouseConfig) {
        let (dx, dy) = accelerate(dx as f32, dy as f32, config);
        self.x += dx;
        self.y += dy;
        self.clamp();
    }

    /// Bring the position back inside the desktop.
    ///
    /// The far edge is `width - 1` and not `width`: a pointer at `width` is on
    /// no pixel of any monitor, so nothing would be under it and the last
    /// column of the screen could never be clicked. A zero-sized desktop
    /// pins it at the origin rather than at −1.
    fn clamp(&mut self) {
        let max_x = self.width.saturating_sub(1) as f32;
        let max_y = self.height.saturating_sub(1) as f32;
        self.x = self.x.clamp(0.0, max_x.max(0.0));
        self.y = self.y.clamp(0.0, max_y.max(0.0));
        if !self.x.is_finite() {
            self.x = 0.0;
        }
        if !self.y.is_finite() {
            self.y = 0.0;
        }
    }
}

/// Scale a raw pointer delta by the user's speed and acceleration settings.
///
/// Speed is geometric — each of the twenty steps is the same *proportional*
/// change, so the slider feels linear to the hand rather than crowding all its
/// effect at one end — and spans 0.25× to 4×, with 0 exactly 1.0.
///
/// Acceleration is one curve with two parameters, and the three profiles are
/// points on it rather than three separate implementations:
///
/// | Profile | Gain | Threshold |
/// |---|---|---|
/// | `Flat` | 0 — no acceleration at any speed | — |
/// | `Adaptive` | 1 | the configured threshold |
/// | `Custom` | the configured gain | the configured threshold |
///
/// Below the threshold nothing is multiplied, so slow, careful movement — the
/// kind used to hit a small target — is exactly as the hand made it. Above it
/// the factor grows with how far over the threshold the movement was, which is
/// what makes crossing a large desktop possible without lifting the mouse.
#[must_use]
fn accelerate(dx: f32, dy: f32, config: &MouseConfig) -> (f32, f32) {
    let base = speed_multiplier(config.speed);
    let (gain, threshold) = match config.accel_profile {
        AccelProfile::Flat => (0.0, 1.0),
        AccelProfile::Adaptive => (1.0, config.accel_threshold as f32),
        AccelProfile::Custom => (config.accel_gain, config.accel_threshold as f32),
    };
    // A zero threshold would divide by zero below. It is user-editable and
    // `validate()` allows 0, so the guard is real rather than defensive
    // paperwork: at a threshold of zero every movement is "over" it, which is
    // the same thing as a flat profile at the gain's multiple.
    let threshold = if threshold > 0.0 { threshold } else { 1.0 };
    let magnitude = dx.hypot(dy);
    let over = (magnitude / threshold).max(1.0) - 1.0;
    let factor = gain.mul_add(over, 1.0).clamp(0.0, MAX_ACCELERATION) * base;
    (dx * factor, dy * factor)
}

/// The multiplier a pointer-speed setting of `speed` means.
#[must_use]
fn speed_multiplier(speed: i32) -> f32 {
    let steps = speed.clamp(-10, 10) as f32 / 5.0;
    let factor = 2.0f32.powf(steps);
    if factor.is_finite() && factor > 0.0 {
        factor
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// Held keys and repeat
// ---------------------------------------------------------------------------

/// One key known to be physically down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    /// Which device reported it. Re-synchronisation after a `SYN_DROPPED` is
    /// per-device — one device's bitmap says nothing about another's keys — so
    /// a key has to remember where it came from or a drop on the mouse would
    /// release everything held on the keyboard.
    device: u32,
    /// The Linux keycode, which is what `EVIOCGKEY`'s bitmap is indexed by.
    keycode: u16,
    /// The scan-code-set-1 code the compositor's keymap wants.
    scancode: u32,
}

/// When the currently-repeating key is next due to repeat.
#[derive(Clone, Copy, Debug)]
struct Repeat {
    /// The key that is repeating, as the compositor names it.
    scancode: u32,
    /// When the next synthesised press is due.
    due: Instant,
}

/// Which keys are down, and which one is repeating.
///
/// One tracker for every device, not one per device: two keyboards plugged into
/// the same machine are one keyboard to the person using them, and Shift held
/// on either must shift a letter typed on the other.
#[derive(Debug, Default)]
struct Keys {
    /// Every key currently believed to be down.
    ///
    /// A `Vec` and not a set because it is searched linearly and never holds
    /// more than the number of keys a person can physically press at once. A
    /// hash of a `u16` would cost more than the scan.
    held: Vec<Held>,
    /// The key repeat is currently being generated for, if any.
    repeat: Option<Repeat>,
}

impl Keys {
    /// Note a key going down, and make it the one that repeats.
    fn press(&mut self, device: u32, keycode: u16, scancode: u32, now: Instant, delay: Duration) {
        if !self
            .held
            .iter()
            .any(|h| h.device == device && h.keycode == keycode)
        {
            self.held.push(Held {
                device,
                keycode,
                scancode,
            });
        }
        // Typematic applies to the key pressed *last*, which is why this
        // replaces rather than adds: holding A and then pressing B repeats B,
        // and releasing B leaves A held but silent — exactly as a real
        // keyboard behaves, because the hardware has one repeat timer.
        if repeats(scancode) {
            self.repeat = Some(Repeat {
                scancode,
                due: now.checked_add(delay).unwrap_or(now),
            });
        }
    }

    /// Note a key coming up, stopping its repeat if it was the one repeating.
    fn release(&mut self, device: u32, keycode: u16) {
        self.held
            .retain(|h| !(h.device == device && h.keycode == keycode));
        if let Some(repeat) = self.repeat
            && !self.held.iter().any(|h| h.scancode == repeat.scancode)
        {
            self.repeat = None;
        }
    }

    /// Emit whatever repeats have come due, and schedule the next.
    fn tick(
        &mut self,
        now: Instant,
        config: &inputsettings::KeyboardRepeatConfig,
    ) -> Vec<InputEvent> {
        let mut out = Vec::new();
        if !config.enabled {
            return out;
        }
        let interval = Duration::from_millis(u64::from(config.repeat_interval_ms.max(1)));
        let Some(repeat) = self.repeat.as_mut() else {
            return out;
        };
        // Bounded, and not `while due <= now`: a tick delayed by a slow frame,
        // a debugger breakpoint or the machine being suspended would otherwise
        // pay out every repeat that "should" have happened while nothing was
        // running, and the user would get a screenful of one letter for having
        // held a key across a hiccup.
        let mut emitted = 0usize;
        while repeat.due <= now && emitted < MAX_REPEATS_PER_TICK {
            out.push(InputEvent::KeyDown {
                scancode: repeat.scancode,
                character: None,
            });
            repeat.due = repeat.due.checked_add(interval).unwrap_or(now);
            emitted = emitted.saturating_add(1);
        }
        if repeat.due <= now {
            // Still behind after the cap: the backlog is discarded rather than
            // carried, so a long stall costs at most one tick's worth of
            // repeats and the key resumes at the ordinary rate.
            repeat.due = now.checked_add(interval).unwrap_or(now);
        }
        out
    }
}

/// Most synthesised repeats emitted for one key in one tick.
///
/// At the fastest setting (10 ms) a 60 Hz frame is worth about two, so four is
/// slack for a frame that ran long without being enough to fill a document from
/// a stall. See [`Keys::tick`].
const MAX_REPEATS_PER_TICK: usize = 4;

/// Whether a key is one that should repeat while held.
///
/// Modifiers and latches must not: a held Shift that repeated would deliver a
/// stream of Shift presses to every client, and a Caps Lock that repeated would
/// toggle itself thirty times a second. Derived from the key's *name* rather
/// than from a second list of scan codes, so that a key added to
/// [`keymap`](crate::keymap) is classified by the table that already knows what
/// it is.
fn repeats(scancode: u32) -> bool {
    !matches!(
        key_for_scancode(scancode),
        Key::LeftShift
            | Key::RightShift
            | Key::LeftCtrl
            | Key::RightCtrl
            | Key::LeftAlt
            | Key::RightAlt
            | Key::LeftSuper
            | Key::RightSuper
            | Key::CapsLock
            | Key::NumLock
            | Key::ScrollLock
    )
}

// ---------------------------------------------------------------------------
// One device's stream
// ---------------------------------------------------------------------------

/// What has accumulated for a device since its last `SYN_REPORT`.
///
/// evdev delivers a coherent group of records terminated by one
/// `EV_SYN`/`SYN_REPORT`: a mouse's horizontal movement, vertical movement and
/// button change arrive as three records and are *one* movement of the hand.
/// Acting on them as they arrive would emit a horizontal-only move followed by
/// a vertical-only one, which draws a staircase instead of a line and makes
/// every diagonal drag jitter.
#[derive(Debug, Default)]
struct Packet {
    /// Accumulated horizontal motion.
    dx: i32,
    /// Accumulated vertical motion.
    dy: i32,
    /// Accumulated vertical scroll, in notches.
    wheel: i32,
    /// Accumulated horizontal scroll, in notches.
    hwheel: i32,
    /// The most recent `MSC_SCAN`, which precedes the `EV_KEY` it belongs to.
    scan: Option<u32>,
    /// Key and button transitions, in the order they arrived, each with the
    /// `MSC_SCAN` that preceded it.
    keys: Vec<(u16, i32, Option<u32>)>,
}

impl Packet {
    /// Whether anything has accumulated.
    fn is_empty(&self) -> bool {
        self.dx == 0 && self.dy == 0 && self.wheel == 0 && self.hwheel == 0 && self.keys.is_empty()
    }

    /// Forget everything accumulated, including the pending scan code.
    fn clear(&mut self) {
        self.dx = 0;
        self.dy = 0;
        self.wheel = 0;
        self.hwheel = 0;
        self.scan = None;
        self.keys.clear();
    }
}

/// One open device, its read buffer and its half-assembled packet.
#[derive(Debug)]
struct Stream<S> {
    /// The device.
    sys: S,
    /// Its `/dev/input/eventN` index, for diagnostics and for keying held keys.
    index: u32,
    /// Its `EVIOCGNAME`, if it answered.
    name: String,
    /// Bytes read but not yet consumed. Non-empty only if a `read` ended
    /// mid-record — which the SlateOS kernel never does, but which this module
    /// is not entitled to assume of every kernel it might run on.
    buf: Vec<u8>,
    /// What has arrived since the last `SYN_REPORT`.
    packet: Packet,
    /// Set by `SYN_DROPPED`; cleared once `EVIOCGKEY` has been re-read.
    needs_resync: bool,
    /// Set when the device failed in a way that will not recover, so it is
    /// skipped rather than re-erroring once a frame for ever.
    dead: bool,
}

impl<S: EventSys> Stream<S> {
    /// Read whatever is waiting and fold it into `out`.
    fn drain(
        &mut self,
        now: Instant,
        settings: &InputSettings,
        keys: &mut Keys,
        pointer: &mut Pointer,
        out: &mut Vec<InputEvent>,
    ) {
        if self.dead {
            return;
        }
        for _ in 0..MAX_READS_PER_TICK {
            let start = self.buf.len();
            self.buf.resize(start.saturating_add(READ_CHUNK), 0);
            let read = match self.sys.read(self.buf.get_mut(start..).unwrap_or(&mut [])) {
                Ok(n) => n,
                Err(EAGAIN) | Err(EINTR) => {
                    // Nothing has happened, or the call was interrupted before
                    // anything did. Both are the ordinary state of an idle
                    // keyboard and neither is a fault.
                    self.buf.truncate(start);
                    break;
                }
                Err(_) => {
                    // Anything else — the device was unplugged, the fd was
                    // revoked — is permanent. Say so once by going quiet,
                    // rather than issuing a failing syscall every frame.
                    self.buf.truncate(start);
                    self.dead = true;
                    return;
                }
            };
            self.buf.truncate(start.saturating_add(read));
            self.consume(now, settings, keys, pointer, out);
            if read < READ_CHUNK {
                // A short read means the device had nothing more to give.
                break;
            }
        }
    }

    /// Decode every whole record in the buffer, leaving any partial tail.
    fn consume(
        &mut self,
        now: Instant,
        settings: &InputSettings,
        keys: &mut Keys,
        pointer: &mut Pointer,
        out: &mut Vec<InputEvent>,
    ) {
        let mut offset = 0usize;
        while let Some(chunk) = self
            .buf
            .get(offset..offset.saturating_add(EVENT_SIZE))
            .and_then(Record::decode)
        {
            offset = offset.saturating_add(EVENT_SIZE);
            self.fold(chunk, now, settings, keys, pointer, out);
        }
        self.buf.drain(..offset.min(self.buf.len()));
    }

    /// Fold one record into the packet, flushing on `SYN_REPORT`.
    fn fold(
        &mut self,
        record: Record,
        now: Instant,
        settings: &InputSettings,
        keys: &mut Keys,
        pointer: &mut Pointer,
        out: &mut Vec<InputEvent>,
    ) {
        match record.kind {
            EV_SYN => match record.code {
                SYN_REPORT => {
                    self.flush(now, settings, keys, pointer, out);
                }
                SYN_DROPPED => {
                    // The half-built packet is now meaningless: some unknown
                    // number of its siblings were never delivered. Everything
                    // after the drop is coherent again, so only what is in hand
                    // is discarded.
                    self.packet.clear();
                    self.needs_resync = true;
                }
                _ => {}
            },
            EV_REL => match record.code {
                REL_X => self.packet.dx = self.packet.dx.saturating_add(record.value),
                REL_Y => self.packet.dy = self.packet.dy.saturating_add(record.value),
                REL_WHEEL => self.packet.wheel = self.packet.wheel.saturating_add(record.value),
                REL_HWHEEL => self.packet.hwheel = self.packet.hwheel.saturating_add(record.value),
                _ => {}
            },
            EV_MSC if record.code == MSC_SCAN => {
                self.packet.scan = u32::try_from(record.value).ok();
            }
            EV_KEY => {
                let scan = self.packet.scan.take();
                self.packet.keys.push((record.code, record.value, scan));
            }
            _ => {}
        }
    }

    /// Turn a complete packet into compositor input events.
    ///
    /// Order matters and is: motion, then buttons, then scroll. A click arrives
    /// in the same packet as the movement that took the pointer to the thing
    /// being clicked, so emitting the button first would deliver it at the
    /// *previous* position — which for a fast click on a small target is a
    /// click on whatever was there before.
    fn flush(
        &mut self,
        now: Instant,
        settings: &InputSettings,
        keys: &mut Keys,
        pointer: &mut Pointer,
        out: &mut Vec<InputEvent>,
    ) {
        if self.packet.is_empty() {
            self.packet.clear();
            return;
        }
        if self.packet.dx != 0 || self.packet.dy != 0 {
            pointer.nudge(self.packet.dx, self.packet.dy, &settings.mouse);
            let (x, y) = pointer.position();
            out.push(InputEvent::MouseMove { x, y });
        }
        let (x, y) = pointer.position();
        // Taken rather than borrowed so that `self` is free for `press`/
        // `release` below, which need `&mut self.packet`'s owner.
        let transitions = std::mem::take(&mut self.packet.keys);
        for (code, value, scan) in transitions {
            if uapi::is_button(code) {
                if let Some(button) = button_for(code, settings.mouse.button_mapping) {
                    match value {
                        KEY_VALUE_DOWN | KEY_VALUE_REPEAT => out.push(InputEvent::MouseButton {
                            button,
                            pressed: true,
                            x,
                            y,
                        }),
                        KEY_VALUE_UP => out.push(InputEvent::MouseButton {
                            button,
                            pressed: false,
                            x,
                            y,
                        }),
                        _ => {}
                    }
                }
                continue;
            }
            let Some(scancode) = scancode_for(code, scan) else {
                continue;
            };
            match value {
                KEY_VALUE_DOWN => {
                    keys.press(
                        self.index,
                        code,
                        scancode,
                        now,
                        Duration::from_millis(u64::from(settings.keyboard.repeat_delay_ms)),
                    );
                    out.push(InputEvent::KeyDown {
                        scancode,
                        character: None,
                    });
                }
                KEY_VALUE_UP => {
                    keys.release(self.index, code);
                    out.push(InputEvent::KeyUp { scancode });
                }
                KEY_VALUE_REPEAT => {
                    // Hardware autorepeat, which SlateOS never sends but a
                    // Linux host does. Passed through as a press without
                    // touching the held set — the key is already down — and
                    // it pushes our own synthesised repeat out of the way, so
                    // that a device with a repeat timer and a compositor with
                    // one do not both pay out.
                    if let Some(repeat) = keys.repeat.as_mut()
                        && repeat.scancode == scancode
                    {
                        let interval = Duration::from_millis(u64::from(
                            settings.keyboard.repeat_interval_ms.max(1),
                        ));
                        repeat.due = now.checked_add(interval).unwrap_or(now);
                    }
                    out.push(InputEvent::KeyDown {
                        scancode,
                        character: None,
                    });
                }
                _ => {}
            }
        }
        if self.packet.wheel != 0 || self.packet.hwheel != 0 {
            let sign = if settings.mouse.natural_scroll {
                -1.0
            } else {
                1.0
            };
            let speed = if settings.mouse.scroll_speed.is_finite() {
                settings.mouse.scroll_speed
            } else {
                1.0
            };
            out.push(InputEvent::MouseScroll {
                dx: self.packet.hwheel as f32 * speed * sign,
                dy: self.packet.wheel as f32 * speed * sign,
                x,
                y,
            });
        }
        self.packet.clear();
    }

    /// Re-read which keys are held, and reconcile both ways.
    ///
    /// Called after a `SYN_DROPPED`. Both directions matter and for different
    /// reasons: a key we think is down and is not would be stuck for ever (a
    /// stuck Shift capitalises everything typed afterwards), and a key that is
    /// down and we do not know about would be missing from the modifier state,
    /// so a Ctrl held across a drop would stop making shortcuts.
    fn resync(&mut self, keys: &mut Keys, out: &mut Vec<InputEvent>) {
        if !self.needs_resync || self.dead {
            return;
        }
        self.needs_resync = false;
        let mut bitmap = [0u8; KEY_BITMAP_BYTES];
        let request = uapi::ioc(uapi::IOC_READ, uapi::EVIOC_NR_GKEY, KEY_BITMAP_BYTES as u32);
        let Ok(len) = self.sys.ioctl_read(request, &mut bitmap) else {
            // The device cannot say. Releasing everything it holds is the safe
            // answer — a key reported up that is really down recovers the
            // moment it is pressed again, whereas one reported down that is
            // really up never recovers at all.
            release_all_from(self.index, keys, out);
            return;
        };
        let bitmap = bitmap.get(..len.min(KEY_BITMAP_BYTES)).unwrap_or(&[]);

        // Down, in our books, but not in the device's: release it.
        let mut released = Vec::new();
        keys.held.retain(|held| {
            if held.device != self.index || uapi::bit_set(bitmap, held.keycode) {
                return true;
            }
            released.push(*held);
            false
        });
        for held in released {
            out.push(InputEvent::KeyUp {
                scancode: held.scancode,
            });
            if keys.repeat.is_some_and(|r| r.scancode == held.scancode) {
                keys.repeat = None;
            }
        }

        // Down in the device's books and not in ours: press it. Only over the
        // range that has a scan code, because a key with none could not be
        // reported to a client anyway.
        for keycode in 0..(KEY_BITMAP_BYTES as u16).saturating_mul(8) {
            if !uapi::bit_set(bitmap, keycode) {
                continue;
            }
            if uapi::is_button(keycode) {
                // Buttons are re-derived from the next packet's transitions;
                // synthesising a press here would deliver a click nobody made.
                continue;
            }
            let Some(scancode) = uapi::set1_for_keycode(keycode) else {
                continue;
            };
            if keys
                .held
                .iter()
                .any(|h| h.device == self.index && h.keycode == keycode)
            {
                continue;
            }
            keys.held.push(Held {
                device: self.index,
                keycode,
                scancode,
            });
            out.push(InputEvent::KeyDown {
                scancode,
                character: None,
            });
        }
    }
}

/// Release every key a device holds, because its state can no longer be known.
fn release_all_from(device: u32, keys: &mut Keys, out: &mut Vec<InputEvent>) {
    let mut released = Vec::new();
    keys.held.retain(|held| {
        if held.device == device {
            released.push(*held);
            false
        } else {
            true
        }
    });
    for held in released {
        out.push(InputEvent::KeyUp {
            scancode: held.scancode,
        });
        if keys.repeat.is_some_and(|r| r.scancode == held.scancode) {
            keys.repeat = None;
        }
    }
}

/// The compositor's scan code for a key event, or `None` if there is none.
///
/// The keycode table comes first and `MSC_SCAN` is the fallback, which is the
/// opposite of what the wire suggests. The reason is that `MSC_SCAN` is not
/// always a set-1 scan code: on a PS/2 device it is (the SlateOS kernel folds
/// the `0xE0` prefix into the high byte exactly as [`keymap`](crate::keymap)
/// expects), but on a USB HID keyboard Linux reports the *HID usage* there
/// instead, and reading one as the other would name a completely different key.
/// The keycode, by contrast, means the same thing on every device.
///
/// A keycode with no set-1 equivalent still yields the raw `MSC_SCAN` if the
/// device sent one, which reaches a client as `Key::Unknown` carrying the code
/// — a media key a remapping utility can bind, rather than a dropped event.
fn scancode_for(keycode: u16, scan: Option<u32>) -> Option<u32> {
    uapi::set1_for_keycode(keycode).or(scan)
}

/// Which logical button an evdev button code is, under the user's mapping.
///
/// `LeftHanded` swaps primary and secondary and leaves the rest alone, which is
/// what the setting means: a left-handed user moves the mouse to the other hand
/// and wants the finger nearest the thumb to be primary. The thumb buttons are
/// not swapped — they are named "back" and "forward" by what they do, not by
/// where they are.
fn button_for(code: u16, mapping: ButtonMapping) -> Option<MouseButton> {
    let swap = mapping == ButtonMapping::LeftHanded;
    Some(match code {
        BTN_LEFT if swap => MouseButton::Right,
        BTN_LEFT => MouseButton::Left,
        BTN_RIGHT if swap => MouseButton::Left,
        BTN_RIGHT => MouseButton::Right,
        BTN_MIDDLE => MouseButton::Middle,
        BTN_SIDE => MouseButton::Back,
        BTN_EXTRA => MouseButton::Forward,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// The input source
// ---------------------------------------------------------------------------

/// Every input device the compositor could open, read as one keyboard and one
/// pointer.
///
/// See the module docs for what it does and does not do. The type is generic
/// over the syscall layer so that all of the above is driven by a fake in
/// tests; [`EvdevInput::open`] is the constructor that uses the real one.
#[derive(Debug)]
pub struct EvdevInput<S> {
    /// One entry per device that opened.
    streams: Vec<Stream<S>>,
    /// Which keys are down, across all of them.
    keys: Keys,
    /// Where the mouse is.
    pointer: Pointer,
    /// The user's pointer and repeat preferences.
    settings: InputSettings,
}

impl<S: EventSys> EvdevInput<S> {
    /// Open every device `source` offers, up to [`MAX_DEVICES`].
    ///
    /// # Errors
    ///
    /// [`EvdevError::Denied`] if every node refused with `EACCES` — the
    /// capability case, which is not this process's to fix — and
    /// [`EvdevError::NoDevices`] if none opened for any other reason.
    pub fn from_source<D: DeviceSource<Sys = S>>(
        source: &mut D,
        settings: InputSettings,
        width: u32,
        height: u32,
    ) -> Result<Self, EvdevError> {
        let mut streams = Vec::new();
        let mut denied = false;
        for index in 0..MAX_DEVICES {
            match source.open(index) {
                Ok(mut sys) => {
                    let name = device_name(&mut sys);
                    streams.push(Stream {
                        sys,
                        index,
                        name,
                        buf: Vec::new(),
                        packet: Packet::default(),
                        needs_resync: false,
                        dead: false,
                    });
                }
                Err(EACCES) => denied = true,
                Err(_) => {}
            }
        }
        if streams.is_empty() {
            return Err(if denied {
                EvdevError::Denied
            } else {
                EvdevError::NoDevices
            });
        }
        Ok(Self {
            streams,
            keys: Keys::default(),
            pointer: Pointer::new(width, height),
            settings,
        })
    }

    /// The devices that opened, as `(index, name)` — for the startup
    /// diagnostic, so that a machine with no keyboard says which nodes it did
    /// find rather than only that typing does not work.
    #[must_use]
    pub fn devices(&self) -> Vec<(u32, &str)> {
        self.streams
            .iter()
            .map(|s| (s.index, s.name.as_str()))
            .collect()
    }

    /// Replace the pointer and repeat preferences.
    ///
    /// Separate from construction because `input.yaml` can change while the
    /// desktop is running — the Settings panel writes it and sends the
    /// compositor a `ReloadInput` — and a pointer speed that only took effect
    /// at the next login would be a setting that appears not to work.
    pub fn set_settings(&mut self, settings: InputSettings) {
        self.settings = settings;
    }

    /// Everything that has happened since the last call, as of `now`.
    ///
    /// Split from [`InputSource::poll`] so that repeat timing — the one part of
    /// this module that depends on the clock rather than on the byte stream —
    /// is testable without sleeping.
    pub fn poll_at(&mut self, now: Instant) -> Vec<InputEvent> {
        let mut out = Vec::new();
        for stream in &mut self.streams {
            stream.drain(
                now,
                &self.settings,
                &mut self.keys,
                &mut self.pointer,
                &mut out,
            );
        }
        // After the reads, not during them: a `SYN_DROPPED` is followed by real
        // events in the same read, and re-querying before those are folded in
        // would reconcile against a state that is already out of date.
        for stream in &mut self.streams {
            stream.resync(&mut self.keys, &mut out);
        }
        out.extend(self.keys.tick(now, &self.settings.keyboard));
        out
    }
}

/// Ask a device its name, for the startup diagnostic.
///
/// A device that will not say is not a device that will not work — the name is
/// for a person reading a log — so a failure yields a placeholder rather than
/// rejecting the device.
fn device_name<S: EventSys>(sys: &mut S) -> String {
    let mut buf = [0u8; NAME_BYTES];
    let request = uapi::ioc(uapi::IOC_READ, uapi::EVIOC_NR_GNAME, NAME_BYTES as u32);
    let Ok(len) = sys.ioctl_read(request, &mut buf) else {
        return String::from("unnamed device");
    };
    let bytes = buf.get(..len.min(NAME_BYTES)).unwrap_or(&[]);
    // The kernel NUL-terminates and includes the NUL in the length.
    let bytes = match bytes.iter().position(|&b| b == 0) {
        Some(end) => bytes.get(..end).unwrap_or(&[]),
        None => bytes,
    };
    if bytes.is_empty() {
        String::from("unnamed device")
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

impl<S: EventSys> InputSource for EvdevInput<S> {
    fn poll(&mut self) -> Vec<InputEvent> {
        self.poll_at(Instant::now())
    }

    fn set_bounds(&mut self, width: u32, height: u32) {
        self.pointer.set_bounds(width, height);
    }

    fn reload_input(&mut self, settings: &InputSettings) {
        self.set_settings(settings.clone());
    }
}

#[cfg(target_os = "linux")]
impl EvdevInput<sys::Device> {
    /// Open the machine's input devices.
    ///
    /// `width` and `height` are the desktop's size, which the pointer is
    /// clamped to and starts at the centre of.
    ///
    /// # Errors
    ///
    /// See [`Self::from_source`]. [`EvdevError::Denied`] is the one worth
    /// reporting specially: it means the compositor was spawned without an
    /// `InputDevice` capability, which it cannot obtain for itself.
    pub fn open(settings: InputSettings, width: u32, height: u32) -> Result<Self, EvdevError> {
        Self::from_source(&mut sys::Devices, settings, width, height)
    }
}

#[cfg(test)]
mod tests;
