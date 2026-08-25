//! Slate OS Compositor — Display Server
//!
//! The central display server for SlateOS. Manages windows, composites their
//! contents onto a framebuffer, and routes input events to the appropriate
//! client windows.
//!
//! # Architecture
//!
//! ```text
//! Client Applications
//!     │ (submit RenderTree via IPC)
//!     ▼
//! Compositor Server
//!     ├── Window Manager (z-order, focus, decorations)
//!     ├── Compositing Pipeline (damage tracking, alpha blending, double buffer)
//!     ├── Input Router (hit testing, event dispatch)
//!     └── Rendering Engine (rasterize RenderCommands to pixels)
//!     │
//!     ▼
//! Framebuffer (presented to display hardware)
//! ```
//!
//! # Design Decisions
//!
//! - Double-buffered compositing with damage tracking to minimize GPU writes.
//! - Window decorations drawn server-side (consistent look, secure close button).
//! - Input routed by Z-order hit testing; keyboard follows focus, mouse follows cursor.
//! - VSync-aware frame scheduling: target refresh rate, skip frames if behind.
//! - Stub IPC layer ready for real Slate OS channel-based IPC when available.

// Drawing primitives (fill_rect, stroke_rect, draw_text, draw_line) and the
// renderer execute() pump take 8-9 args (framebuffer + geometry + color +
// optional clip / font / weight / stroke-width). Grouping into a struct
// would help marginally but obscures the per-call clarity at the call site
// — every primitive needs every arg.
#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub use appearance::{AppearanceSettings, WindowCorners};
#[allow(unused_imports)]
use guitk::color::Color;
// Aliased because this crate has its own `MouseButton` and `MouseEventKind`
// describing the *hardware* side of the same concepts. They are genuinely
// different types — the compositor's carry no coordinates and no Enter/Leave,
// which are properties of a window rather than of a device — and letting the
// names collide would make every conversion below ambiguous to a reader.
use guitk::event::{
    Event as ClientEvent, Key, KeyEvent as ClientKeyEvent, Modifiers,
    MouseButton as ClientMouseButton, MouseEvent as ClientMouseEvent,
    MouseEventKind as ClientMouseKind,
};
#[allow(unused_imports)]
use guitk::render::{
    FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow, TextSpan,
};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use osfont::raster::GlyphMask;
use osfont::system::{Family, FontCache, Weight};

mod buffer;
pub use buffer::{BufferFormat, SharedBuffer};
// The rendering-backend seam. Everything from `compose_frame` down to a
// primitive is written against `RenderTarget`, so the CPU rasterizer below is a
// *choice* rather than the only thing the compositor can do — which is what a
// GPU backend needs in order to exist at all. See the module docs.
mod render;
pub use render::{RenderBackend, RenderTarget};
mod keymap;
pub use keymap::{ModifierState, key_for_scancode};
// The one piece of keyboard state that spans two events. `keymap` is a pure
// function of a single keystroke, and a dead key is the case that cannot be:
// `´` types nothing until the `e` after it arrives. Kept beside `keymap` and
// not inside it so that the pure part stays pure.
mod deadkey;
// The front end that turns a byte stream from a client into compositor calls
// and compositor events back into bytes. Everything above this line works in
// terms of typed requests; `wire` is the only place that parses frames.
mod wire;
pub use wire::{ClientLink, WireError};
// The listening front end that owns the sockets `wire` deliberately does not.
// `wire` is the translation, `server` is the plumbing; keeping them apart is
// what lets the translation be tested without a network.
mod server;
pub use server::{Disconnect, Server, ServerStats};
// Where a finished frame goes and where input comes from. Everything above this
// line composites into a buffer nothing looked at, and routes input that never
// arrived; `present` is the one seam that closes both, because both had the
// same cause — the compositor owned no device.
pub mod present;
pub use present::{Headless, Present, Recording};
// Remote draw-command streaming uses the shared `guiremote` crate's scene
// protocol (multi-window deltas built on its single-window RenderCommand wire
// codec), rather than a compositor-local duplicate.
// The cursor shape a client asks for and the cursor shape the compositor
// displays were two separate enums that had to agree, and did not: the wire
// one carried `Help` and the compositor's did not, so a client asking for a
// context-help cursor got silently nothing. There is one type now, and it is
// the wire's — the compositor is the end of that pipe, not a parallel
// vocabulary for the same thing.
pub use guiremote::control::CursorShape;
// Re-exported because `Window::layer` is public and a caller reading it needs
// to be able to name the type without depending on `guiremote` directly.
pub use guiremote::control::Layer;
// Same reason: `CompositorRequest::ShellControl` carries one, so a caller
// building that request must be able to name it.
pub use guiremote::control::ShellControlAction;
use guiremote::control::WindowSpec;
// Re-exported for the same reason as `WindowInfo` below: `Window::reserved_edge`
// holds one and `reserve_edge` takes one, and a panel that has to reach past the
// compositor to name the edge it is anchored to is naming a different type from
// the one it is talking to. The rules that go with it — how strips add up and
// how far a client may push — live in `guiremote::reserve`, so that a panel and
// the compositor cannot disagree about what a reservation means.
pub use guiremote::reserve::PanelEdge;
use guiremote::reserve::ReservedEdges;
use guiremote::scene::{SceneFrame, SceneSession, WindowSnapshot};
// Same reason: `window_list` returns these, and a shell reading one should not
// have to reach past the compositor to name what it got.
pub use guiremote::window_list::{WindowInfo, WindowList};
// Same reason again: `Window::snapped` can hold one, and `snap_window_to_zone`
// takes one. The type belongs to the protocol crate because both ends have to
// agree on which rectangle a slot names — see `guiremote::zones`.
use guiremote::zones::{EdgeDrop, SnapZone, WorkArea};
pub use guiremote::zones::{SnapLayoutPreset, SnapSlot};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Height of the window title bar in pixels.
const TITLE_BAR_HEIGHT: u32 = 30;

/// Width of the window border in pixels.
const BORDER_WIDTH: u32 = 1;

/// Size of the window shadow in pixels.
const SHADOW_SIZE: u32 = 8;

/// Width/height of title bar buttons (close, maximize, minimize).
const TITLE_BUTTON_SIZE: u32 = 20;

/// Spacing between title bar buttons.
const TITLE_BUTTON_SPACING: u32 = 4;

// The double-click window's default and its permitted range are taken from
// `inputsettings`, which is the crate that owns the file the value is read
// from, rather than restated here. They were restated here until 2026-08-22,
// and the copies did agree — but nothing anywhere would have failed if one of
// them had moved, and the whole point of the reload path added alongside this
// is that the number the user chose in Settings, the number the file clamps to,
// and the number this compares two timestamps against are one number.
use inputsettings::{
    DEFAULT_DOUBLE_CLICK_MS, InputSettings, MAX_DOUBLE_CLICK_MS, MIN_DOUBLE_CLICK_MS,
};

/// Default window opacity (fully opaque).
const DEFAULT_OPACITY: f32 = 1.0;

/// Smallest client-area width the compositor will resize a window to.
///
/// A hard floor beneath any client-supplied minimum: a window narrower than
/// its own title-bar buttons cannot be closed, moved or resized by the user,
/// so honouring a client's request for one would hand it a way to strand a
/// window on the desktop.
const MIN_WINDOW_WIDTH: u32 = 100;

/// The frame interval implied by a refresh rate in Hz.
///
/// Zero means the display never reported one — an EDID-less panel, a headless
/// target — and gets ~60 Hz rather than a division by zero. Written as
/// `checked_div` so the zero case is handled *by the division*: the two call
/// sites this replaces each guarded it separately and then disagreed about the
/// answer, one saying `Duration::from_millis(16)` (62.5 Hz) and the other
/// `Duration::from_micros(16_667)` (60 Hz). That is what a rule kept in two
/// places does, and it is why the guard now lives with the operation.
const fn frame_interval_for(refresh_rate: u32) -> Duration {
    Duration::from_micros(match 1_000_000u64.checked_div(refresh_rate as u64) {
        Some(interval) => interval,
        None => 16_667,
    })
}

/// Smallest client-area height the compositor will resize a window to. See
/// [`MIN_WINDOW_WIDTH`].
const MIN_WINDOW_HEIGHT: u32 = 50;

/// Maximum framebuffer width supported.
const MAX_FB_WIDTH: u32 = 7680;

/// Maximum framebuffer height supported.
const MAX_FB_HEIGHT: u32 = 4320;

// ---------------------------------------------------------------------------
// Window ID generation
// ---------------------------------------------------------------------------

/// Global atomic counter for generating unique window IDs.
static NEXT_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// Allocate a new unique window ID.
    fn allocate() -> Self {
        Self(NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw numeric value.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Reconstitute an id from the number a client sent back over the wire.
    ///
    /// Deliberately not `From<u64>`: this is not a conversion, it is the point
    /// where an untrusted number is *claimed* to name a window. Nothing here
    /// checks that it does — every use is followed by a lookup that returns
    /// [`CompositorError::WindowNotFound`] if it does not, and the ugly name is
    /// there to make sure a caller notices it must do that.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Compositor error type.
#[derive(Clone, Debug)]
pub enum CompositorError {
    /// The specified window does not exist.
    WindowNotFound(WindowId),
    /// Invalid framebuffer dimensions.
    InvalidDimensions { width: u32, height: u32 },
    /// The framebuffer exceeds maximum supported size.
    FramebufferTooLarge { width: u32, height: u32 },
    /// A rendering operation failed.
    RenderError(String),
    /// IPC communication error.
    IpcError(String),
    /// Display configuration error.
    DisplayError(String),
    /// A client supplied an invalid shared buffer (bad geometry/stride/size).
    InvalidBuffer(String),
    /// A client shared buffer exceeds the supported pixel cap.
    BufferTooLarge { width: u32, height: u32 },
    /// The referenced remote stream session id is not active.
    StreamNotFound(u64),
    /// A resize was asked of a window the client declared non-resizable.
    NotResizable(WindowId),
    /// A snap layout produced no rectangle for the zone that was requested.
    ///
    /// Not reachable through [`SnapSlot`], which cannot be built naming a zone
    /// its layout does not have. It exists because the alternative to reporting
    /// the failure is inventing a rectangle, and a window silently placed
    /// *somewhere* is a worse outcome than a request that visibly failed.
    ZoneNotInLayout(WindowId),
}

impl std::fmt::Display for CompositorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowNotFound(id) => write!(f, "window not found: {}", id.raw()),
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid dimensions: {}x{}", width, height)
            }
            Self::FramebufferTooLarge { width, height } => {
                write!(
                    f,
                    "framebuffer too large: {}x{} (max {}x{})",
                    width, height, MAX_FB_WIDTH, MAX_FB_HEIGHT
                )
            }
            Self::RenderError(msg) => write!(f, "render error: {}", msg),
            Self::IpcError(msg) => write!(f, "ipc error: {}", msg),
            Self::DisplayError(msg) => write!(f, "display error: {}", msg),
            Self::InvalidBuffer(msg) => write!(f, "invalid shared buffer: {}", msg),
            Self::BufferTooLarge { width, height } => write!(
                f,
                "shared buffer too large: {}x{} (max {}x{})",
                width, height, MAX_FB_WIDTH, MAX_FB_HEIGHT
            ),
            Self::StreamNotFound(id) => write!(f, "stream session not found: {}", id),
            Self::NotResizable(id) => write!(f, "window is not resizable: {}", id.raw()),
            Self::ZoneNotInLayout(id) => {
                write!(f, "snap layout has no such zone: {}", id.raw())
            }
        }
    }
}

pub type CompositorResult<T> = Result<T, CompositorError>;

// ---------------------------------------------------------------------------
// Geometry types
// ---------------------------------------------------------------------------

/// A 2D point (screen coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A 2D rectangle (screen coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The distance from `lo` to `hi` as an extent, zero when `hi` is not past `lo`.
///
/// `(hi - lo) as u32` is the obvious spelling and it is wrong twice over. Screen
/// coordinates are `i32` and a client picks them — `CreateWindow` and
/// `SetPosition` both carry a position straight off the wire — so `hi - lo`
/// overflows for a pair as far apart as `i32::MIN` and `i32::MAX`, which is a
/// panic *in the display server*, i.e. every application on the desktop dies
/// because one of them asked for a silly window position. And the true distance
/// between those two does not fit in an `i32` at all, so even a checked
/// subtraction could only fail, never answer.
///
/// `wrapping_sub` answers exactly. For `hi > lo` the mathematical difference is
/// always in `0..=u32::MAX`, and two's-complement subtraction reproduces its low
/// 32 bits — which, given the value fits, *is* the value. The `hi <= lo` case is
/// separated out because an empty span is a legitimate result (two rectangles
/// that merely touch), not an error.
#[allow(clippy::cast_sign_loss)] // The bit pattern is the answer; see above.
const fn span(lo: i32, hi: i32) -> u32 {
    if hi <= lo {
        0
    } else {
        hi.wrapping_sub(lo) as u32
    }
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Grow by `amount` pixels on every side.
    ///
    /// Saturating for the same reason [`Window::outer_rect`] is: a rectangle
    /// already at the coordinate edge stays pinned there rather than wrapping
    /// to the opposite extreme.
    pub fn inflate(&self, amount: u32) -> Rect {
        let signed = i32::try_from(amount).unwrap_or(i32::MAX);
        Rect::new(
            self.x.saturating_sub(signed),
            self.y.saturating_sub(signed),
            self.width.saturating_add(amount.saturating_mul(2)),
            self.height.saturating_add(amount.saturating_mul(2)),
        )
    }

    /// The same rectangle moved by `(dx, dy)`.
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.x.saturating_add(dx),
            self.y.saturating_add(dy),
            self.width,
            self.height,
        )
    }

    /// The exclusive right edge: the first column *past* the rectangle.
    ///
    /// `try_from` rather than `as`, because a width above `i32::MAX` would cast
    /// to a negative number and put the right edge to the *left* of the origin
    /// — a rectangle that contains nothing at all. Pinning at `i32::MAX`
    /// instead keeps "very wide" meaning very wide.
    pub fn right(&self) -> i32 {
        self.x
            .saturating_add(i32::try_from(self.width).unwrap_or(i32::MAX))
    }

    /// The exclusive bottom edge: the first row *past* the rectangle.
    pub fn bottom(&self) -> i32 {
        self.y
            .saturating_add(i32::try_from(self.height).unwrap_or(i32::MAX))
    }

    /// Check if a point is inside this rectangle.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }

    /// Compute the intersection of two rectangles. Returns None if they don't overlap.
    ///
    /// The far edges come from [`right`](Self::right) and
    /// [`bottom`](Self::bottom) rather than being recomputed here, because those
    /// two saturate a width above `i32::MAX` instead of casting it — and `as
    /// i32` on such a width yields a *negative* number, so the far edge lands
    /// left of the near one and a gigantic rectangle intersects to nothing.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());

        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, span(x1, x2), span(y1, y2)))
        } else {
            None
        }
    }

    /// Compute the bounding box that contains both rectangles.
    ///
    /// Same reasoning as [`intersect`](Self::intersect), and it bites harder
    /// here: with `as i32`, unioning an over-wide rectangle produced a bounding
    /// box *smaller* than either input, so a caller sizing a buffer from the
    /// union would have believed an impossible desktop fitted.
    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());

        Rect::new(x1, y1, span(x1, x2), span(y1, y2))
    }

    /// The part of `self` not covered by `other`, as up to four **disjoint**
    /// rectangles (top band, bottom band, then left and right of the overlap).
    ///
    /// Disjointness is the property the occlusion cull depends on, not merely a
    /// nicety: a window is redrawn once per surviving fragment, so a pixel
    /// appearing in two fragments would be painted twice — invisible for an
    /// opaque fill and wrong for a translucent one (the shadow would darken
    /// where fragments met). Cutting bands full-width first and only then
    /// splitting the middle row guarantees no pixel is emitted twice.
    ///
    /// Returns `self` unchanged when the two do not overlap, and nothing at all
    /// when `other` covers `self` entirely.
    pub fn subtract(&self, other: &Rect) -> Vec<Rect> {
        let Some(i) = self.intersect(other) else {
            return vec![*self];
        };
        let mut out = Vec::with_capacity(4);
        let (sx0, sy0) = (self.x, self.y);
        let sx1 = self.x.saturating_add(self.width as i32);
        let sy1 = self.y.saturating_add(self.height as i32);
        let (ix0, iy0) = (i.x, i.y);
        let ix1 = i.x.saturating_add(i.width as i32);
        let iy1 = i.y.saturating_add(i.height as i32);

        if iy0 > sy0 {
            out.push(Rect::new(sx0, sy0, self.width, span(sy0, iy0)));
        }
        if sy1 > iy1 {
            out.push(Rect::new(sx0, iy1, self.width, span(iy1, sy1)));
        }
        let mid_h = span(iy0, iy1);
        if mid_h > 0 {
            if ix0 > sx0 {
                out.push(Rect::new(sx0, iy0, span(sx0, ix0), mid_h));
            }
            if sx1 > ix1 {
                out.push(Rect::new(ix1, iy0, span(ix1, sx1), mid_h));
            }
        }
        out
    }
}

/// Subtract every rectangle in `occluders` from `base`, yielding a disjoint
/// cover of the part of `base` that survives.
///
/// Returns `None` when the result would exceed `max_parts` fragments. That is a
/// deliberate bail-out rather than a failure: each fragment costs one full
/// replay of a window's command list, so past a handful of them the cull is
/// buying fewer pixels than the replays cost. The caller then draws the window
/// unclipped, which is always correct — the cull is an optimization, and it is
/// allowed to decline.
fn subtract_region(base: Rect, occluders: &[Rect], max_parts: usize) -> Option<Vec<Rect>> {
    let mut parts = vec![base];
    for occ in occluders {
        let mut next = Vec::with_capacity(parts.len());
        for p in &parts {
            next.extend(p.subtract(occ));
        }
        if next.len() > max_parts {
            return None;
        }
        parts = next;
        if parts.is_empty() {
            break;
        }
    }
    Some(parts)
}

// ---------------------------------------------------------------------------
// Input events (compositor-level)
// ---------------------------------------------------------------------------

/// Mouse button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Input event received from the input subsystem.
#[derive(Clone, Debug)]
pub enum InputEvent {
    /// Mouse moved to absolute position.
    MouseMove { x: i32, y: i32 },
    /// Mouse button pressed or released.
    MouseButton {
        button: MouseButton,
        pressed: bool,
        x: i32,
        y: i32,
    },
    /// Mouse scroll wheel.
    MouseScroll { dx: f32, dy: f32, x: i32, y: i32 },
    /// Key pressed.
    KeyDown {
        scancode: u32,
        character: Option<char>,
    },
    /// Key released.
    KeyUp { scancode: u32 },
    /// Text input (after IME processing).
    TextInput { text: String },
}

// ---------------------------------------------------------------------------
// Snap-zone geometry
// ---------------------------------------------------------------------------

/// The compositor's own bounds, described the way `guiremote::zones` wants them.
///
/// The conversion is a widening, not a narrowing: display coordinates are in
/// the low tens of thousands at most, comfortably inside the 24-bit range where
/// `f32` represents every integer exactly, so no coordinate is disturbed by the
/// round trip through floating point.
#[allow(
    clippy::cast_precision_loss,
    reason = "display coordinates are far inside f32's exact-integer range"
)]
fn work_area_of(bounds: Rect) -> WorkArea {
    WorkArea::new(
        bounds.x as f32,
        bounds.y as f32,
        bounds.width as f32,
        bounds.height as f32,
    )
}

/// What letting go of a dragged window would do, **and on which monitor**.
///
/// The work area travels with the intent rather than being looked up again at
/// the release, because the two lookups need not agree. The preview is drawn
/// from the monitor under the pointer; the window is dragged along with the
/// pointer but not necessarily *onto* the same monitor — it straddles the seam
/// while crossing it, and it lags the pointer by however far along its title bar
/// it was grabbed. A release that re-derived the area from the window would then
/// tile it on the wrong screen, after showing an outline on the right one.
/// Carrying the area makes that divergence unrepresentable rather than merely
/// tested against.
///
/// It is also what the preview's damage bookkeeping needs: a preview whose
/// monitor changed has to be painted over where it *was*, and the outgoing
/// rectangle cannot be recovered from the drop alone.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DropIntent {
    /// What the drop means — maximize, or a named zone.
    drop: EdgeDrop,
    /// The monitor's work area the drop was aimed at, which the release resolves
    /// `drop` against.
    area: WorkArea,
}

/// A work area back in whole pixels.
///
/// The inverse of [`work_area_of`] for any area that came from one, since every
/// coordinate a display produces survives the round trip through `f32` exactly.
/// It exists because the tiling operations divide between two vocabularies:
/// `guiremote::zones` speaks `WorkArea`, and the compositor's own geometry —
/// `client_geometry_for_frame`, `Rect::intersect` — speaks `Rect`.
fn work_rect(area: WorkArea) -> Rect {
    let (x, y) = (round_px(area.x), round_px(area.y));
    Rect::new(
        x,
        y,
        u32::try_from(round_px(area.right()).saturating_sub(x)).unwrap_or(0),
        u32::try_from(round_px(area.bottom()).saturating_sub(y)).unwrap_or(0),
    )
}

/// `rect` moved by the smallest amount that puts it inside `bounds`.
///
/// The size is never changed, so a rectangle too large to fit cannot be made to
/// fit; it is pinned at the top-left corner instead. That choice is not
/// arbitrary — for a window frame the top-left is where the title bar is, and
/// any other anchor would push the title bar off the top or left edge and leave
/// the window exactly as ungrabbable as it started.
fn pulled_onto(rect: Rect, bounds: Rect) -> Rect {
    let last_x = bounds
        .right()
        .saturating_sub(i32::try_from(rect.width).unwrap_or(i32::MAX))
        .max(bounds.x);
    let last_y = bounds
        .bottom()
        .saturating_sub(i32::try_from(rect.height).unwrap_or(i32::MAX))
        .max(bounds.y);
    Rect::new(
        rect.x.clamp(bounds.x, last_x),
        rect.y.clamp(bounds.y, last_y),
        rect.width,
        rect.height,
    )
}

/// A window frame placed so that some of it is on a screen: `frame` unchanged if
/// any part of it already falls on `desktop`, and otherwise pulled onto
/// `fallback`.
///
/// Two rectangles rather than one because the questions are different. *Can the
/// user see this window?* is asked of the whole virtual desktop — a window
/// sitting happily on the second monitor is not stranded merely because the
/// first one shrank, and yanking it across would be the more visible bug. *Where
/// should a window nobody can see go?* has to name one screen that actually
/// exists, and the union of several monitors is not necessarily one: an
/// L-shaped arrangement has a hole in its bounding box, and a window placed in
/// the hole is exactly as lost as it was before.
///
/// The intersection test is strict — *no part of the frame is on any screen* —
/// on purpose. A window hanging half off an edge is still visible and still has
/// a title bar to grab, so its owner's choice of position stands; only a window
/// with nothing on screen at all has no title bar, cannot be dragged, and cannot
/// be reached with the pointer.
fn kept_reachable(frame: Rect, desktop: Rect, fallback: Rect) -> Rect {
    if frame.intersect(&desktop).is_some() {
        frame
    } else {
        pulled_onto(frame, fallback)
    }
}

/// One zone edge, rounded to a whole pixel.
///
/// The clamp is what makes the cast safe rather than merely likely to be: the
/// value handed to `as` is already inside `i32`'s range, so there is nothing to
/// truncate. A `NaN` cannot arise from a finite work area, and if one somehow
/// did, Rust's float-to-int casts saturate rather than invoke undefined
/// behaviour — it would become `0`, a wrong pixel but never a wild one.
#[allow(
    clippy::cast_possible_truncation,
    reason = "clamped into i32's range before the cast"
)]
fn round_px(v: f32) -> i32 {
    v.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// How much of the desktop shows through the drop preview.
///
/// A wash rather than an opaque pane: the preview covers whatever the user is
/// about to tile over, and hiding it would mean choosing a destination without
/// being able to see what is already there.
const PREVIEW_FILL_OPACITY: f32 = 0.25;

/// How thick the drop preview's border is.
const PREVIEW_BORDER_PX: u32 = 2;

/// The four bands making up a `thickness`-pixel border just inside `rect`.
///
/// Returned as rectangles rather than drawn, so a caller can paint them
/// however it likes — and so the four are guaranteed **disjoint**: the top and
/// bottom bands span the full width and the side bands fill only what is left
/// between them. Overlapping them would draw the corners twice, which for the
/// translucent preview would leave four visibly darker squares.
///
/// Degenerate rectangles are handled by construction rather than rejected, and
/// the two bands on an axis are sized in turn rather than symmetrically: the
/// far band gets only what the near one left. A rect shorter than twice the
/// thickness would otherwise produce a top and a bottom band that *overlap* in
/// the middle — the same double-blend the disjointness is there to prevent,
/// showing up on exactly the small rectangles where it is most visible. So a
/// three-pixel-tall rect with a two-pixel border yields bands of two and one,
/// and a two-pixel-tall one a single band covering it with an empty partner.
fn rect_outline(rect: Rect, thickness: u32) -> [Rect; 4] {
    let top_h = thickness.min(rect.height);
    let bottom_h = thickness.min(rect.height.saturating_sub(top_h));
    let left_w = thickness.min(rect.width);
    let right_w = thickness.min(rect.width.saturating_sub(left_w));
    let inner_h = rect.height.saturating_sub(top_h).saturating_sub(bottom_h);
    let offset = |base: i32, by: u32| base.saturating_add(i32::try_from(by).unwrap_or(i32::MAX));
    let inner_y = offset(rect.y, top_h);
    [
        Rect::new(rect.x, rect.y, rect.width, top_h),
        Rect::new(
            rect.x,
            offset(rect.y, rect.height.saturating_sub(bottom_h)),
            rect.width,
            bottom_h,
        ),
        Rect::new(rect.x, inner_y, left_w, inner_h),
        Rect::new(
            offset(rect.x, rect.width.saturating_sub(right_w)),
            inner_y,
            right_w,
            inner_h,
        ),
    ]
}

/// A zone's rectangle in the compositor's whole-pixel coordinates.
///
/// Each of the four *edges* is rounded, and the extents are then derived from
/// the rounded edges. Rounding the origin and the size independently is the
/// obvious alternative and it tiles badly: two zones that share a boundary
/// would round it twice, once as one zone's right edge and once as the other's
/// left, and the two answers can differ — which shows up as a one-pixel column
/// that either belongs to both windows or to neither.
fn zone_rect(zone: SnapZone) -> Rect {
    let left = round_px(zone.x);
    let top = round_px(zone.y);
    let right = round_px(zone.x + zone.width);
    let bottom = round_px(zone.y + zone.height);
    Rect::new(left, top, span(left, right), span(top, bottom))
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// Which half of the work area a snapped window fills.
///
/// The *edge* is what is stored, not the rectangle it currently implies. A
/// stored rectangle would be wrong the moment the display resolution changed or
/// a monitor was unplugged, and would be wrong silently — the window would keep
/// filling half of a screen that no longer exists. Storing the intent means the
/// geometry is re-derived from the current bounds, which is the same reason
/// `maximized` is a flag rather than a saved full-screen rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapEdge {
    /// The left half.
    Left,
    /// The right half.
    Right,
}

/// What a snapped window is snapped *to*.
///
/// Two ways of asking for a tiled position, kept as separate variants because
/// they resolve to genuinely different rectangles rather than being two names
/// for one. [`Half`](Self::Half) splits the display at its midpoint with no
/// gutter, so two windows on opposite halves meet with no visible seam — that
/// is what the keyboard shortcut has always done. [`Zone`](Self::Zone) is one
/// cell of a named layout offered by the shell's zone picker, and every cell of
/// a layout is inset by [`guiremote::zones::ZONE_GAP`] so the tiled windows
/// read as separate panes. Folding `Half` into `Zone(TwoEqualHalves, 0 | 1)`
/// would look like a simplification and would in fact hand every keyboard snap
/// a six-pixel gutter it never asked for.
///
/// Like [`SnapEdge`], and for the same reason spelled out there, this stores
/// the *request* and never the rectangle the request currently resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapTarget {
    /// One half of the display, with no gutter between the halves.
    Half(SnapEdge),
    /// One cell of a named multi-window layout, inset by the zone gap.
    Zone(SnapSlot),
}

/// A managed window in the compositor.
#[derive(Clone, Debug)]
pub struct Window {
    /// Unique identifier for this window.
    pub id: WindowId,
    /// Window title (displayed in title bar).
    pub title: String,
    /// Position of the window's top-left corner (including decorations).
    pub x: i32,
    /// Position of the window's top-left corner (including decorations).
    pub y: i32,
    /// Width of the client area in pixels.
    pub width: u32,
    /// Height of the client area in pixels.
    pub height: u32,
    /// Whether the window is visible.
    pub visible: bool,
    /// Whether the window is minimized to the taskbar.
    pub minimized: bool,
    /// Whether the window is maximized (fills the screen).
    pub maximized: bool,
    /// Whether this window currently has keyboard focus.
    pub focused: bool,
    /// Z-order index (higher = more in front).
    pub z_order: u32,
    /// Which band of the stacking order this window may move within.
    ///
    /// Fixed at creation and never changed afterwards. See [`Layer`] for why
    /// the band is a role rather than a starting z-order: a starting depth is
    /// something the first raise destroys.
    pub layer: Layer,
    /// Window opacity (0.0 = fully transparent, 1.0 = fully opaque).
    pub opacity: f32,
    /// Process ID of the client that owns this window.
    pub client_pid: u64,
    /// The most recently submitted render tree from the client.
    pub render_tree: RenderTree,
    /// An attached shared pixel buffer (DMA-BUF path). When `Some`, the
    /// compositor blits these pixels into the client area instead of replaying
    /// `render_tree`; the client renders directly into shared memory.
    pub buffer: Option<SharedBuffer>,
    /// Whether the window is in true fullscreen mode: it owns the entire
    /// display with no decorations. Distinct from `maximized` (which keeps the
    /// title bar/borders and respects panel reservations). Fullscreen is the
    /// state that enables direct-scanout bypass for games/video.
    pub fullscreen: bool,
    /// Geometry saved before entering fullscreen (for restore). Kept separate
    /// from `restore_rect` so fullscreen and maximize don't clobber each other.
    pub fs_restore_rect: Option<Rect>,
    /// Position and size before maximizing *or snapping* (for restore).
    pub restore_rect: Option<Rect>,
    /// The strip along a monitor edge this window has reserved for itself, if
    /// any: a taskbar or dock keeping tiled windows out of the pixels it sits
    /// in. `None` for the overwhelming majority of windows, which are not
    /// panels.
    ///
    /// **Stored on the window rather than in a table beside it** for the same
    /// reason [`SnapTarget`] is: the two questions a reservation raises are
    /// *which monitor* and *for how long*, and a window already answers both.
    /// The monitor is the one it overlaps most, by the same rule everything
    /// else uses; the lifetime is the window's own, so a panel that is
    /// destroyed or whose client hangs up releases its claim with no bookkeeping
    /// that could be forgotten. A separate `HashMap<WindowId, _>` would be a
    /// second lifetime to keep in step with this one, and the failure mode of
    /// getting it wrong is a permanent strip of unusable desktop with nothing
    /// left on screen to release it from.
    ///
    /// Counted only while the window is visible and not minimized — see
    /// [`Compositor::reserved_on`].
    pub reserved_edge: Option<(PanelEdge, u32)>,
    /// What this window is snapped to, if anything.
    ///
    /// A separate state from [`maximized`](Self::maximized) rather than a
    /// second flag beside it, because the three are mutually exclusive and a
    /// pair of booleans can represent a window that is both — which
    /// [`restore_window`](Compositor::restore_window) would then have to pick
    /// between. `Option` makes the illegal state unrepresentable and gives
    /// "snapped, and to where" one answer instead of two.
    pub snapped: Option<SnapTarget>,
    /// Whether the window needs to be redrawn.
    pub dirty: bool,
    /// Whether the compositor draws a title bar and borders for this window.
    ///
    /// A menu, a tooltip and a splash screen are all windows that must not get
    /// a title bar; before this field existed the compositor drew one on
    /// everything, so a client had no way to ask for a bare surface at all.
    pub decorations: bool,
    /// Whether the user may resize the window by dragging its border.
    pub resizable: bool,
    /// Whether the client draws its own background.
    ///
    /// When set, the compositor skips the opaque white client-area fill, so
    /// whatever the client does not paint shows what is behind it. Distinct
    /// from [`opacity`](Self::opacity), which fades the *whole* window
    /// uniformly including its decorations.
    pub transparent: bool,
    /// Smallest client area the window may be resized to, if it named one.
    pub min_size: Option<(u32, u32)>,
    /// Largest client area the window may be resized to, if it named one.
    pub max_size: Option<(u32, u32)>,
    /// The cursor this window asks for while the pointer is over its client
    /// area.
    ///
    /// Per-window, not global: the shape belongs to what is under the pointer.
    /// A text editor setting an I-beam must not change the cursor over the
    /// file manager next to it, which is exactly what a single compositor-wide
    /// shape did — any client could repaint the desktop cursor from anywhere.
    pub cursor: CursorShape,
    /// The scale factor of the display this window mostly sits on.
    ///
    /// **Decorations only.** It scales the title bar, borders, buttons and
    /// shadow — the parts the compositor draws. It deliberately does *not*
    /// scale `width`/`height`, which are the client area in physical pixels and
    /// are the client's to choose: a client is told its display's scale over
    /// the wire and decides for itself whether to render larger or to render
    /// the same content at more pixels. Scaling the client area here would
    /// resize windows behind their owners' backs.
    ///
    /// Recomputed from scratch by
    /// [`Compositor::refresh_window_scales`](Compositor::refresh_window_scales)
    /// before every frame and every input event, rather than being bumped by
    /// each of the several places that move a window — see that method for why.
    /// 1.0 until the compositor first refreshes it, which is what makes this
    /// field invisible on a single 96dpi display.
    pub scale_factor: f32,
    /// Which virtual desktop this window belongs to.
    ///
    /// Meaningful only for [`Layer::Normal`] windows: see
    /// [`Window::is_showing`]. A window is created on whichever workspace was
    /// being shown at the time, which is where a window that just opened
    /// belongs.
    ///
    /// **Held here rather than in the shell** because the shell cannot act on
    /// it: hiding a window it does not own would need a verb that lets any
    /// shell hide any client's window, which is exactly the ambient authority
    /// the `ShellControl` design exists to avoid. The compositor already owns
    /// every other reason a window is not on screen (`visible`, `minimized`),
    /// and a switch is then one recomposite rather than N requests. See
    /// `design-decisions.md` §518.
    pub workspace: u32,
}

/// Scale a decoration dimension to physical pixels, never rounding a visible
/// dimension away to nothing.
///
/// `BORDER_WIDTH` is 1, so any scale under 1.5 rounds it to 1 or 0 — and 0 is
/// not a thin border, it is a window whose edge cannot be grabbed to resize it,
/// because hit testing derives from the same number. Anything the unscaled
/// value made non-zero stays non-zero; anything already zero (the undecorated
/// case) stays zero, since inventing a 1px title bar for a tooltip would be
/// worse than the bug being guarded against.
fn scale_dimension(value: u32, scale: f32) -> u32 {
    if value == 0 {
        return 0;
    }
    // `max(1.0)` before the cast, not `.max(1)` after: the cast saturates at 0
    // for a negative or NaN scale, and a `u32` max cannot tell that apart from
    // a legitimately tiny result.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a sane range before the cast; the cast's own \
                  saturation is what the clamp exists to pre-empt"
    )]
    let scaled = (value as f32 * scale).round().max(1.0) as u32;
    scaled
}

impl Window {
    /// Create a window on the terms a client asked for.
    ///
    /// `x`/`y` are passed separately rather than read from `spec.position`
    /// because placement is the compositor's decision: a spec with no position
    /// gets one chosen for it, and even one *with* a position may be overridden
    /// by a tiling policy. By the time this is called the argument has been
    /// settled either way.
    fn from_spec(spec: &WindowSpec, x: i32, y: i32, client_pid: u64) -> Self {
        Self {
            id: WindowId::allocate(),
            title: spec.title.clone(),
            x,
            y,
            width: spec.width,
            height: spec.height,
            visible: true,
            minimized: false,
            maximized: false,
            focused: false,
            z_order: 0,
            layer: spec.layer,
            opacity: DEFAULT_OPACITY,
            client_pid,
            render_tree: RenderTree::new(),
            buffer: None,
            fullscreen: false,
            fs_restore_rect: None,
            restore_rect: None,
            reserved_edge: None,
            snapped: None,
            dirty: true,
            decorations: spec.decorations,
            resizable: spec.resizable,
            transparent: spec.transparent,
            min_size: spec.min_size,
            max_size: spec.max_size,
            cursor: CursorShape::Arrow,
            // 1.0 until the compositor places the window and learns which
            // display that put it on. A `Window` on its own has no way to know:
            // the display list lives on `DisplayManager`, and the honest
            // default is the one that leaves geometry exactly as it was before
            // this field existed.
            scale_factor: 1.0,
            // Overwritten by `create_window_from_spec` with the workspace being
            // shown. A `Window` on its own has no way to know which that is,
            // and 0 is the workspace a compositor that never switches stays on.
            workspace: 0,
        }
    }

    /// Whether this window is on screen right now.
    ///
    /// The one predicate for "the user can see this and click on it", which
    /// every render pass, every hit test and every occlusion cull asks instead
    /// of spelling the conditions out. It was three separate spellings of
    /// `visible && !minimized` across twelve call sites before workspaces
    /// existed; adding a third reason a window is hidden to twelve places
    /// independently is how a window ends up invisible but still taking clicks.
    ///
    /// **A window outside [`Layer::Normal`] is on every workspace.** The
    /// wallpaper and the shell's chrome — taskbar, start menu, the switcher
    /// overlay — are furniture belonging to the *screen*, not documents
    /// belonging to a desktop, and a taskbar that vanished when you switched
    /// desktop would take the only means of switching back with it. Using the
    /// layer for this rather than a separate `sticky` flag keeps it a property
    /// of what a surface *is*: the same distinction the shell already makes
    /// when it drops non-`Normal` windows from its taskbar.
    #[must_use]
    pub const fn is_showing(&self, current_workspace: u32) -> bool {
        self.visible
            && !self.minimized
            && (!matches!(self.layer, Layer::Normal) || self.workspace == current_workspace)
    }

    /// Whether this window is drawn with a frame right now.
    ///
    /// Fullscreen suppresses decorations regardless of what the client asked
    /// for — a fullscreen window owns the entire display, and a title bar
    /// floating over a game is not a thing anyone wants. The client's
    /// `decorations` request is remembered, not overwritten, so leaving
    /// fullscreen restores the frame.
    pub const fn is_framed(&self) -> bool {
        self.decorations && !self.fullscreen
    }

    /// How much space the frame takes on each side of the client area, in
    /// pixels: `(top, side, bottom)`. All zero for an unframed window.
    ///
    /// Every piece of geometry below derives from this rather than reading
    /// `TITLE_BAR_HEIGHT`/`BORDER_WIDTH` directly, so an undecorated window is
    /// undecorated everywhere — hit testing, damage and drag detection
    /// included — instead of only where someone remembered to check.
    ///
    /// It is also the single place [`scale_factor`](Self::scale_factor) is
    /// applied to the frame, for the same reason: a title bar drawn at 2× and
    /// hit-tested at 1× is a title bar the user can see but not drag.
    ///
    /// No longer `const` — `f32` arithmetic is not permitted in a `const fn`.
    /// Nothing depended on it being one.
    pub fn frame_insets(&self) -> (u32, u32, u32) {
        if self.is_framed() {
            (
                scale_dimension(TITLE_BAR_HEIGHT, self.scale_factor),
                scale_dimension(BORDER_WIDTH, self.scale_factor),
                scale_dimension(BORDER_WIDTH, self.scale_factor),
            )
        } else {
            (0, 0, 0)
        }
    }

    /// How far the drop shadow extends beyond the frame. Zero when unframed:
    /// the shadow is part of the decoration, not of the window.
    pub fn shadow_extent(&self) -> u32 {
        if self.is_framed() {
            scale_dimension(SHADOW_SIZE, self.scale_factor)
        } else {
            0
        }
    }

    /// Get the total bounds including decorations (title bar, borders, shadow).
    ///
    /// Saturating throughout, because the position and size are the client's to
    /// choose and the frame is added to them: a window at `i32::MIN` grown by a
    /// border must stay pinned at the edge of the coordinate space. Wrapping
    /// would teleport it to the opposite extreme and panicking would let one
    /// client's bad request kill the whole display server.
    pub fn outer_rect(&self) -> Rect {
        self.frame_rect().inflate(self.shadow_extent())
    }

    /// The decorated box: client area plus title bar and borders, *without* the
    /// drop shadow. [`outer_rect`](Self::outer_rect) is this inflated by
    /// [`shadow_extent`](Self::shadow_extent).
    ///
    /// Named once because four places used to spell it out from the constants
    /// and had begun to disagree — `render_shadow` derived it one way,
    /// `render_border` another, and `window_drawn_extent` carried a fourth copy
    /// under a comment admitting it "mirrors the geometry in `render_shadow`".
    /// A box that four functions each recompute is a box that will be drawn in
    /// one place and hit-tested in another.
    pub fn frame_rect(&self) -> Rect {
        self.frame_rect_for_client(self.client_rect())
    }

    /// The frame box a *given* client rectangle would have on this window — the
    /// inverse of [`client_geometry_for_frame`](Self::client_geometry_for_frame),
    /// and the general form of [`frame_rect`](Self::frame_rect).
    ///
    /// Needed because a saved rectangle is stored as client geometry
    /// (`restore_rect`, `fs_restore_rect`) while every question about whether a
    /// window can be *reached* is a question about its decorated box: it is the
    /// title bar, not the client area, that has to be on screen to be grabbed.
    /// Asking that of a window that is not currently at the rectangle in
    /// question is the whole point, so it cannot be `frame_rect`.
    pub fn frame_rect_for_client(&self, client: Rect) -> Rect {
        let (top, side, bottom) = self.frame_insets();
        Rect::new(
            client.x.saturating_sub(side as i32),
            client.y.saturating_sub(top as i32),
            client.width.saturating_add(side.saturating_mul(2)),
            client.height.saturating_add(top).saturating_add(bottom),
        )
    }

    /// Get the client area rectangle (where the application draws).
    pub fn client_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    /// The client geometry `(x, y, width, height)` whose *frame* box is exactly
    /// `area` — the inverse of [`frame_rect`](Self::frame_rect).
    ///
    /// This is what "maximise" means: it is the window's decorations, not its
    /// client area, that are made flush with the edges of the work area. Stated
    /// once, as the inverse, because a maximise that insets by its own copy of
    /// the constants drifts the moment the frame changes — and it is the frame
    /// that the user sees touching the screen edge.
    ///
    /// Saturating in both directions: an `area` smaller than the frame it must
    /// hold yields a zero-size client area rather than a wrapped enormous one.
    /// The caller runs the result through [`clamp_size`](Self::clamp_size)
    /// anyway, which is where a minimum size is reasserted.
    pub fn client_geometry_for_frame(&self, area: Rect) -> (i32, i32, u32, u32) {
        let (top, side, bottom) = self.frame_insets();
        (
            area.x
                .saturating_add(i32::try_from(side).unwrap_or(i32::MAX)),
            area.y
                .saturating_add(i32::try_from(top).unwrap_or(i32::MAX)),
            area.width.saturating_sub(side.saturating_mul(2)),
            area.height.saturating_sub(top).saturating_sub(bottom),
        )
    }

    /// A screen point expressed in this window's client coordinates.
    ///
    /// One method rather than the ten copies of `x - win.x` this replaces,
    /// because the subtraction is not as safe as it looks: the window origin is
    /// the *client's* to choose, so a window near `i32::MAX` and a pointer near
    /// `i32::MIN` overflow it — and this sits on the input path, where the
    /// consequence is the display server dying on a mouse move.
    ///
    /// Saturating is the right answer rather than merely the safe one: the
    /// result says "very far outside this window", which is exactly what a
    /// point that distant is. Every consumer either hit-tests it (and rejects
    /// it) or forwards it to a client that does.
    pub fn local_point(&self, x: i32, y: i32) -> (i32, i32) {
        (x.saturating_sub(self.x), y.saturating_sub(self.y))
    }

    /// Get the title bar rectangle (for drag and button hit testing), or
    /// `None` for a window that has no title bar.
    ///
    /// `Option` rather than an empty rect because every caller hit-tests
    /// against it, and an empty rect contains no point only by accident of
    /// arithmetic — the type should force the undecorated case to be
    /// considered rather than have it fall out right by luck.
    pub fn title_bar_rect(&self) -> Option<Rect> {
        if !self.is_framed() {
            return None;
        }
        let top = self.frame_insets().0;
        let frame = self.frame_rect();
        Some(Rect::new(frame.x, frame.y, frame.width, top))
    }

    /// The rectangle of the title-bar button in the given slot, counting from
    /// the right-hand edge: slot 0 is the rightmost.
    ///
    /// Slots rather than a chain (`minimize` positioned off `maximize`, off
    /// `close`) because a window that cannot be maximised has no maximize
    /// button, and a chain would leave a hole where it used to be instead of
    /// letting minimize move up into the vacated slot.
    fn title_button_rect(&self, slot: u32) -> Option<Rect> {
        let title_rect = self.title_bar_rect()?;
        // The bar's width and origin are the client's to influence, so the
        // whole chain is saturating: a button positioned off the coordinate
        // space is one the user cannot click, where an overflow here is the
        // display server dying while drawing a title bar.
        //
        // Scaled alongside the bar that contains them: buttons left at 20px
        // inside a 60px title bar would sit in its top-left corner with the
        // centring arithmetic below pushing them further off as the bar grows.
        let size = scale_dimension(TITLE_BUTTON_SIZE, self.scale_factor);
        let spacing = scale_dimension(TITLE_BUTTON_SPACING, self.scale_factor);
        let step = (size.saturating_add(spacing)) as i32;
        let btn_x = title_rect
            .x
            .saturating_add(title_rect.width as i32)
            .saturating_sub(size as i32)
            .saturating_sub(spacing as i32)
            .saturating_sub((slot as i32).saturating_mul(step));
        let btn_y = title_rect
            .y
            .saturating_add((title_rect.height as i32).saturating_sub(size as i32) / 2);
        Some(Rect::new(btn_x, btn_y, size, size))
    }

    /// Get the close button rectangle, or `None` when there is no title bar.
    pub fn close_button_rect(&self) -> Option<Rect> {
        self.title_button_rect(0)
    }

    /// Get the maximize button rectangle, or `None` when there is no title bar
    /// or the window cannot be resized.
    ///
    /// Maximising is a resize, so a window that declared itself non-resizable
    /// does not get the button at all — drawing one that refuses to work is
    /// worse than not drawing it.
    pub fn maximize_button_rect(&self) -> Option<Rect> {
        if !self.resizable {
            return None;
        }
        self.title_button_rect(1)
    }

    /// Get the minimize button rectangle, or `None` when there is no title bar.
    ///
    /// Minimising is always available: it does not change the window's size,
    /// only whether it is on screen.
    pub fn minimize_button_rect(&self) -> Option<Rect> {
        self.title_button_rect(if self.resizable { 2 } else { 1 })
    }

    /// The title bar and the buttons on it, as one value.
    ///
    /// Gathered up front so the renderer can be handed the same rectangles the
    /// hit test uses without borrowing the window across the call — the
    /// compositor's render methods take `&mut self`, and the window lives in
    /// `self.windows`.
    pub fn title_bar_layout(&self) -> Option<TitleBarLayout> {
        Some(TitleBarLayout {
            frame: self.frame_rect(),
            bar: self.title_bar_rect()?,
            close: self.close_button_rect(),
            maximize: self.maximize_button_rect(),
            minimize: self.minimize_button_rect(),
            scale: self.scale_factor,
        })
    }

    /// Clamp a requested client-area size to the window's declared limits.
    ///
    /// The floor of `MIN_WINDOW_WIDTH`/`MIN_WINDOW_HEIGHT` applies even to a
    /// window that asked for something smaller: a window with no title bar
    /// visible enough to grab is one the user cannot recover.
    ///
    /// A `max_size` below the corresponding `min_size` is contradictory; the
    /// minimum wins, because a window too small to use is worse than one
    /// larger than it asked to be.
    pub fn clamp_size(&self, width: u32, height: u32) -> (u32, u32) {
        let (min_w, min_h) = self.min_size.unwrap_or((0, 0));
        let min_w = min_w.max(MIN_WINDOW_WIDTH);
        let min_h = min_h.max(MIN_WINDOW_HEIGHT);
        let (max_w, max_h) = self.max_size.unwrap_or((u32::MAX, u32::MAX));
        (
            width.clamp(min_w, max_w.max(min_w)),
            height.clamp(min_h, max_h.max(min_h)),
        )
    }
}

/// Where a window's title bar and its buttons are on screen.
///
/// One value so that drawing and hit testing cannot drift apart: both read
/// these rectangles, rather than each deriving the same arithmetic from the
/// window's origin and the button constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBarLayout {
    /// The whole decorated box — [`Window::frame_rect`] — that the bar sits at
    /// the top of. Carried alongside the bar so the shadow, the border and the
    /// bar are all drawn from one measurement of the window.
    pub frame: Rect,
    /// The bar itself, spanning the window's full framed width.
    pub bar: Rect,
    /// The close button. Always present on a title bar.
    pub close: Option<Rect>,
    /// The maximize button, absent for a non-resizable window.
    pub maximize: Option<Rect>,
    /// The minimize button. Always present on a title bar.
    pub minimize: Option<Rect>,
    /// The display scale the rectangles above were measured at — a copy of
    /// [`Window::scale_factor`].
    ///
    /// Carried rather than passed alongside, for the reason in this struct's
    /// own doc comment: the renderer needs it for the several decoration
    /// dimensions that are *not* rectangles — the shadow's layer count and cast
    /// offset, the border stroke width, the title font size, the text inset —
    /// and threading it as a second argument would let a caller hand one
    /// function's rectangles to another function's scale.
    pub scale: f32,
}

impl TitleBarLayout {
    /// How many buttons this bar actually carries.
    pub const fn button_count(&self) -> u32 {
        (self.close.is_some() as u32)
            .saturating_add(self.maximize.is_some() as u32)
            .saturating_add(self.minimize.is_some() as u32)
    }
}

// ---------------------------------------------------------------------------
// Damage tracking
// ---------------------------------------------------------------------------

/// A region of the screen that needs to be redrawn.
#[derive(Clone, Debug, Default)]
pub struct DamageRegion {
    /// List of dirty rectangles.
    rects: Vec<Rect>,
}

impl DamageRegion {
    /// Create an empty damage region.
    pub fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// Mark a rectangle as damaged (needing redraw).
    pub fn add(&mut self, rect: Rect) {
        // Merge with existing rects if they overlap to avoid excessive redraw regions.
        for existing in &mut self.rects {
            if existing.intersect(&rect).is_some() {
                *existing = existing.union(&rect);
                return;
            }
        }
        self.rects.push(rect);
    }

    /// Mark the entire screen as damaged.
    pub fn mark_full(&mut self, width: u32, height: u32) {
        self.rects.clear();
        self.rects.push(Rect::new(0, 0, width, height));
    }

    /// Check if there is any damage to process.
    pub fn has_damage(&self) -> bool {
        !self.rects.is_empty()
    }

    /// Get all damaged rectangles.
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Clear all damage (after compositing).
    pub fn clear(&mut self) {
        self.rects.clear();
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

/// Double-buffered framebuffer for compositing.
pub struct Framebuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Back buffer (being composited into).
    back: Vec<u32>,
    /// Front buffer (currently being displayed).
    front: Vec<u32>,
    /// Screen-space rectangle every drawing primitive is confined to, or `None`
    /// for the whole framebuffer.
    ///
    /// This is the occlusion cull's enforcement point (BENCH-COMPOSITOR-SLOW).
    /// It lives on the framebuffer rather than in `RenderEngine`'s clip stack
    /// because a window is painted by three routes that do not share that stack
    /// — the render engine's commands, the decoration helpers, and the
    /// shared-buffer blit — and a cull that only one of them honoured would
    /// silently let a hidden window's decorations through. Every primitive that
    /// writes a pixel intersects with this, so there is one place to be right.
    frame_clip: Option<Rect>,
}

impl Framebuffer {
    /// Create a new framebuffer with the given dimensions.
    pub fn new(width: u32, height: u32) -> CompositorResult<Self> {
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidDimensions { width, height });
        }
        if width > MAX_FB_WIDTH || height > MAX_FB_HEIGHT {
            return Err(CompositorError::FramebufferTooLarge { width, height });
        }

        // Bounded by the MAX_FB_* check just above; saturating so the bound is
        // enforced by the code rather than only by the reader.
        let size = (width as usize).saturating_mul(height as usize);
        Ok(Self {
            width,
            height,
            back: vec![0xFF_00_00_00; size], // Opaque black
            front: vec![0xFF_00_00_00; size],
            frame_clip: None,
        })
    }

    /// Confine every subsequent drawing primitive to `clip` (screen space).
    ///
    /// `None` restores the whole framebuffer. The background clear
    /// ([`clear`](Self::clear) / [`clear_except`](Self::clear_except)) is
    /// deliberately *not* clipped: it runs once per frame before any window and
    /// has its own, separate cull.
    fn set_frame_clip(&mut self, clip: Option<Rect>) {
        self.frame_clip = clip;
    }

    /// Resolve the horizontal span `[x_start, x_end)` of row `y` against the
    /// framebuffer bounds and the active [`frame_clip`](Self::frame_clip).
    ///
    /// Returns `None` when the row is outside either, which is the whole of the
    /// occlusion cull's saving in the row primitives.
    #[inline]
    fn clip_span(&self, y: u32, x_start: u32, x_end: u32) -> Option<(u32, u32)> {
        if y >= self.height || x_end <= x_start {
            return None;
        }
        let (mut lo, mut hi) = (x_start, x_end.min(self.width));
        if let Some(c) = self.frame_clip.as_ref() {
            let cy0 = c.y.max(0) as u32;
            let cy1 = c.y.saturating_add(c.height as i32).max(0) as u32;
            if y < cy0 || y >= cy1 {
                return None;
            }
            let cx0 = c.x.max(0) as u32;
            let cx1 = c.x.saturating_add(c.width as i32).max(0) as u32;
            lo = lo.max(cx0);
            hi = hi.min(cx1);
        }
        if hi <= lo { None } else { Some((lo, hi)) }
    }

    /// Whether the single pixel (`x`, `y`) is inside the bounds and the clip.
    #[inline]
    fn clip_allows(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        match self.frame_clip.as_ref() {
            Some(c) => c.contains(x as i32, y as i32),
            None => true,
        }
    }

    /// Index of pixel (`x`, `y`) in a row-major buffer of `stride` pixels/row.
    ///
    /// Saturating, and that is not a compromise: **every** index this produces
    /// is consumed by `get`/`get_mut`, so a saturated one names no pixel and is
    /// declined — exactly what already happens to an out-of-range one. A
    /// wrapped one would name a real pixel in the wrong place and silently
    /// scramble the frame, which is far worse than a dropped write.
    ///
    /// In practice it cannot saturate: [`new`](Self::new) and
    /// [`resize`](Self::resize) cap the dimensions at
    /// [`MAX_FB_WIDTH`] × [`MAX_FB_HEIGHT`] ≈ 33 Mpx, which fits in a `u32`.
    /// But that cap is enforced two functions away, so the proof lives here —
    /// one place to re-check if the cap ever moves — rather than being redone
    /// at each of the dozen sites that used to compute this inline.
    #[inline]
    const fn pixel_index(stride: usize, x: usize, y: usize) -> usize {
        y.saturating_mul(stride).saturating_add(x)
    }

    /// The `get`/`get_mut` range covering columns `[x_lo, x_hi)` of row `y`.
    ///
    /// Empty when `x_hi <= x_lo`, which slicing handles without a guard.
    #[inline]
    const fn row_range(
        stride: usize,
        y: usize,
        x_lo: usize,
        x_hi: usize,
    ) -> std::ops::Range<usize> {
        let lo = Self::pixel_index(stride, x_lo, y);
        lo..lo.saturating_add(x_hi.saturating_sub(x_lo))
    }

    /// One channel of a source-over blend: `src` at alpha `a`, over `dst`.
    ///
    /// The per-pixel alpha bound lives here, once, instead of being re-assumed
    /// by the three channel expressions in each of the three blend loops.
    /// Taking `u8`s is the whole trick: `src*a + dst*(255-a)` is then at most
    /// 255 × 255 = 65 025 whatever the caller passes, so the saturating forms
    /// below can never actually saturate. The old `u32` call sites had the same
    /// bound but only by convention, and a caller that broke it would have
    /// panicked the display server rather than been rejected by the compiler.
    #[inline]
    const fn blend_channel(src: u8, dst: u8, a: u8) -> u32 {
        let a = a as u32;
        let inv = 255u32.saturating_sub(a);
        (src as u32)
            .saturating_mul(a)
            .saturating_add((dst as u32).saturating_mul(inv))
            / 255
    }

    /// How many whole scanlines of `width` pixels a row-band chunk holds.
    ///
    /// `width` is never zero — [`new`](Self::new) and [`resize`](Self::resize)
    /// both reject a zero dimension — but the division is written total anyway,
    /// because "the constructor rejects it" is an invariant three call frames
    /// away and a zero here would take down the display server.
    #[inline]
    fn rows_in(chunk: &[u32], width: u32) -> u32 {
        let rows = chunk.len().checked_div(width as usize).unwrap_or(0);
        u32::try_from(rows).unwrap_or(u32::MAX)
    }

    /// The effective source alpha of `color` drawn at `opacity`, as the byte
    /// the blend math wants.
    ///
    /// Returning `u8` rather than `u32` is the point: this is the one place the
    /// "alpha is a byte" bound is established, so [`blend_channel`] can take it
    /// from the type instead of from a comment at every call site.
    #[inline]
    fn effective_alpha(color: u32, opacity: f32) -> u8 {
        let raw = ((color >> 24) & 0xFF) as f32;
        (raw * opacity).clamp(0.0, 255.0) as u8
    }

    /// Swap front and back buffers.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Clear the back buffer to a solid color.
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): a full 4K clear writes ~33 MB, enough that a
    /// single core does not saturate DRAM write bandwidth. Above
    /// [`PARALLEL_FILL_THRESHOLD_PX`] the fill is split into disjoint row-bands
    /// filled on scoped worker threads (`split_at_mut` guarantees each thread
    /// owns a non-overlapping `&mut [u32]`, so this is safe with no `unsafe`).
    /// Below the threshold, or when the platform reports no usable parallelism
    /// (e.g. a target whose std has not implemented `available_parallelism`), it
    /// falls back to a single-threaded `[u32]::fill` — so this never pessimizes
    /// small buffers or single-core targets.
    pub fn clear(&mut self, color: u32) {
        let workers = Self::fill_worker_count(self.back.len());
        if workers <= 1 {
            self.back.fill(color);
            return;
        }
        let band_stride = self.back.len().div_ceil(workers);
        std::thread::scope(|s| {
            for chunk in self.back.chunks_mut(band_stride) {
                s.spawn(move || chunk.fill(color));
            }
        });
    }

    /// Number of worker threads to use for a solid fill covering `px` pixels.
    ///
    /// Returns 1 (single-threaded) for small fills or when the platform cannot
    /// report parallelism, so callers can use the result to decide whether to
    /// spawn threads at all. Capped at 8 to bound per-frame thread-spawn cost.
    fn fill_worker_count(px: usize) -> usize {
        // ~1M px (e.g. > 1024×1024). Below this the thread-spawn overhead is not
        // worth it; the per-frame spawn cost would dominate the fill savings.
        const PARALLEL_FILL_THRESHOLD_PX: usize = 1 << 20;
        if px < PARALLEL_FILL_THRESHOLD_PX {
            return 1;
        }
        std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
            .min(8)
    }

    /// Fill `buf` — which holds `band_rows` contiguous scanlines of `width`
    /// pixels each, the first of which is at absolute framebuffer row `y0` — with
    /// `color`, skipping the horizontal spans covered by any `covered` rect.
    ///
    /// Shared by the single-threaded and parallel [`clear_except`] paths so the
    /// per-scanline span-merging logic lives in exactly one place. `covered`
    /// rects are given in absolute framebuffer coordinates; the vertical overlap
    /// test uses the absolute row `y0 + r`, and writes target the band-local row
    /// offset `r * width`.
    fn fill_uncovered_band(
        buf: &mut [u32],
        y0: u32,
        band_rows: u32,
        width: u32,
        color: u32,
        covered: &[Rect],
        fb_height: u32,
    ) {
        let width_usize = width as usize;
        // Reused across scanlines so this allocates once, not per row.
        let mut spans: Vec<(u32, u32)> = Vec::with_capacity(covered.len());
        for r in 0..band_rows {
            let abs_y = y0.saturating_add(r);
            spans.clear();
            for rect in covered {
                let ry0 = rect.y.max(0) as u32;
                let ry1 = (rect.y.saturating_add(rect.height as i32).max(0) as u32).min(fb_height);
                if abs_y >= ry0 && abs_y < ry1 {
                    let x0 = rect.x.max(0) as u32;
                    let x1 = (rect.x.saturating_add(rect.width as i32).max(0) as u32).min(width);
                    if x1 > x0 {
                        spans.push((x0, x1));
                    }
                }
            }
            let row = r as usize;
            if spans.is_empty() {
                if let Some(s) = buf.get_mut(Self::row_range(width_usize, row, 0, width_usize)) {
                    s.fill(color);
                }
                continue;
            }
            // Sort covered spans by start, then fill the complementary gaps.
            spans.sort_unstable_by_key(|&(a, _)| a);
            let mut cursor = 0u32;
            for &(a, b) in &spans {
                if a > cursor {
                    let gap = Self::row_range(width_usize, row, cursor as usize, a as usize);
                    if let Some(s) = buf.get_mut(gap) {
                        s.fill(color);
                    }
                }
                cursor = cursor.max(b);
            }
            if cursor < width {
                let tail = Self::row_range(width_usize, row, cursor as usize, width_usize);
                if let Some(s) = buf.get_mut(tail) {
                    s.fill(color);
                }
            }
        }
    }

    /// Clear a specific rectangle in the back buffer.
    pub fn clear_rect(&mut self, rect: &Rect, color: u32) {
        let x_start = rect.x.max(0) as u32;
        let y_start = rect.y.max(0) as u32;
        let x_end = (rect.x.saturating_add(rect.width as i32) as u32).min(self.width);
        let y_end = (rect.y.saturating_add(rect.height as i32) as u32).min(self.height);

        // Per row rather than per pixel: the span is contiguous, so one `fill`
        // does what a column loop of bounds-checked single stores did, and
        // lowers to a memset instead of `width` branches.
        let stride = self.width as usize;
        for row in y_start..y_end {
            let span = Self::row_range(stride, row as usize, x_start as usize, x_end as usize);
            if let Some(s) = self.back.get_mut(span) {
                s.fill(color);
            }
        }
    }

    /// Clear the back buffer to `color`, but skip pixels covered by any rect in
    /// `covered`.
    ///
    /// The caller guarantees every `covered` rect will be fully overwritten by
    /// opaque content later in the frame, so clearing those pixels first is pure
    /// overdraw. Skipping it is bit-identical to a full [`clear`](Self::clear)
    /// followed by the opaque paints, but writes fewer bytes — the dominant cost
    /// on a full 4K recomposite is memory bandwidth (BENCH-COMPOSITOR-SLOW).
    ///
    /// Correctness: because the covered regions are opaque, their final pixels
    /// never depend on what the background clear wrote (opaque paint replaces,
    /// and any translucent window above blends against the opaque pixel, not the
    /// cleared background). Rects are clipped to the framebuffer; overlapping and
    /// unsorted rects are handled by per-scanline span merging.
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): culls the desktop-background clear under
    /// fully-opaque covering windows. Per-scanline interval math is O(rows ×
    /// covered) which is negligible next to the pixel stores it elides.
    pub fn clear_except(&mut self, color: u32, covered: &[Rect]) {
        if covered.is_empty() {
            // Delegates to the (possibly parallel) full-buffer clear.
            self.clear(color);
            return;
        }
        let width = self.width;
        let height = self.height;
        let workers = Self::fill_worker_count(self.back.len());
        if workers <= 1 {
            Self::fill_uncovered_band(&mut self.back, 0, height, width, color, covered, height);
            return;
        }
        // Partition the scanlines into `workers` disjoint row-bands. Each band is
        // a non-overlapping `&mut [u32]` (via chunks_mut), so the scoped threads
        // never alias — safe parallel fill with no `unsafe`.
        let rows_per_band = height.div_ceil(workers as u32);
        let band_stride = Self::pixel_index(width as usize, 0, rows_per_band as usize);
        std::thread::scope(|s| {
            for (band_idx, chunk) in self.back.chunks_mut(band_stride).enumerate() {
                let y0 = (band_idx as u32).saturating_mul(rows_per_band);
                let band_rows = Self::rows_in(chunk, width);
                s.spawn(move || {
                    Self::fill_uncovered_band(chunk, y0, band_rows, width, color, covered, height);
                });
            }
        });
    }

    /// Set a pixel in the back buffer (bounds-checked).
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if self.clip_allows(x, y) {
            let idx = Self::pixel_index(self.width as usize, x as usize, y as usize);
            if let Some(pixel) = self.back.get_mut(idx) {
                *pixel = color;
            }
        }
    }

    /// Get a pixel from the back buffer (bounds-checked).
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.width && y < self.height {
            let idx = Self::pixel_index(self.width as usize, x as usize, y as usize);
            self.back.get(idx).copied()
        } else {
            None
        }
    }

    /// Blend a pixel with alpha onto the back buffer at the given position.
    #[inline]
    pub fn blend_pixel(&mut self, x: u32, y: u32, src_color: u32, window_opacity: f32) {
        if !self.clip_allows(x, y) {
            return;
        }

        let idx = Self::pixel_index(self.width as usize, x as usize, y as usize);
        let dst = match self.back.get(idx) {
            Some(&val) => val,
            None => return,
        };

        let src_a = Self::effective_alpha(src_color, window_opacity);

        if src_a == 255 {
            // Fully opaque — just write.
            if let Some(pixel) = self.back.get_mut(idx) {
                *pixel = src_color | 0xFF_00_00_00;
            }
            return;
        }
        if src_a == 0 {
            return;
        }

        // `as u8` is the channel extraction: it keeps the low byte, which is
        // exactly what the `>> n & 0xFF` pairs used to spell out.
        let out_r = Self::blend_channel((src_color >> 16) as u8, (dst >> 16) as u8, src_a);
        let out_g = Self::blend_channel((src_color >> 8) as u8, (dst >> 8) as u8, src_a);
        let out_b = Self::blend_channel(src_color as u8, dst as u8, src_a);

        if let Some(pixel) = self.back.get_mut(idx) {
            *pixel = 0xFF_00_00_00 | (out_r << 16) | (out_g << 8) | out_b;
        }
    }

    /// Copy an opaque run of source pixels into back-buffer row `y` starting at
    /// column `x`, clipped to the framebuffer.
    ///
    /// This is the fast path for blitting opaque content: it does no per-pixel
    /// blending or bounds checking — the visible sub-run is resolved once and
    /// written with a single `copy_from_slice`. Negative `x` (partially
    /// offscreen on the left) and right-edge overflow are clipped by slicing.
    pub fn copy_row(&mut self, x: i32, y: u32, src: &[u32]) {
        if y >= self.height || src.is_empty() {
            return;
        }
        // Resolve the source offset (when clipped on the left) and destination
        // column, both within bounds.
        let (src_off, dst_x) = if x < 0 {
            let skip = x.unsigned_abs() as usize;
            if skip >= src.len() {
                return;
            }
            (skip, 0usize)
        } else {
            (0usize, x as usize)
        };
        if dst_x >= self.width as usize {
            return;
        }
        // Both subtractions are guarded above (`skip >= src.len()` and
        // `dst_x >= width` both returned), so saturating is exact here.
        let count = src
            .len()
            .saturating_sub(src_off)
            .min((self.width as usize).saturating_sub(dst_x));
        if count == 0 {
            return;
        }
        // Narrow the destination span to the occlusion clip, then walk the
        // source forward by however much the left edge moved, so the two stay
        // in step (a clip that trimmed only the destination would smear the
        // source sideways).
        let dst_end = u32::try_from(dst_x.saturating_add(count)).unwrap_or(u32::MAX);
        let Some((x_lo, x_hi)) = self.clip_span(y, dst_x as u32, dst_end) else {
            return;
        };
        // `clip_span` only ever narrows, so x_lo >= dst_x and x_hi > x_lo; the
        // saturating forms are exact and merely make that visible.
        let src_off = src_off.saturating_add((x_lo as usize).saturating_sub(dst_x));
        let count = (x_hi.saturating_sub(x_lo)) as usize;
        let row = Self::row_range(
            self.width as usize,
            y as usize,
            x_lo as usize,
            x_hi as usize,
        );
        if let (Some(dst), Some(s)) = (
            self.back.get_mut(row),
            src.get(src_off..src_off.saturating_add(count)),
        ) {
            dst.copy_from_slice(s);
        }
    }

    /// Blit the opaque fast path of a client buffer onto the back buffer,
    /// parallelizing across destination row-bands on multicore hosts.
    ///
    /// Copies `rows` source scanlines of `cols` opaque pixels each from `buf`,
    /// placing the top-left source pixel at framebuffer coordinate
    /// (`win_x`, `win_y`). Left-clipping (negative `win_x`), right-edge clipping,
    /// and vertical clipping are byte-identical to running
    /// [`copy_row`](Self::copy_row) per source row (the previous serial path).
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): a maximized buffer-backed window is the
    /// dominant per-frame blit cost, and the opaque path is pure per-row
    /// `copy_from_slice`s that are independent across destination rows. Above
    /// [`fill_worker_count`](Self::fill_worker_count)'s threshold the framebuffer
    /// is partitioned into disjoint row-bands (`chunks_mut` — non-overlapping
    /// `&mut [u32]`, safe with no `unsafe`) and each band is filled on a scoped
    /// worker. `buf` is shared read-only (`&SharedBuffer` is `Sync`); each worker
    /// only writes rows that fall inside its own band. Below the threshold, or on
    /// single-core targets, it runs single-threaded — no pessimization.
    fn blit_opaque(&mut self, buf: &SharedBuffer, win_x: i32, win_y: i32, cols: u32, rows: u32) {
        let width = self.width;
        let height = self.height;
        // Work proportional to the visible pixel count; reuse the fill heuristic.
        let workers = Self::fill_worker_count((rows as usize).saturating_mul(cols as usize));
        let clip = self.frame_clip;
        if workers <= 1 {
            Self::blit_opaque_band(
                &mut self.back,
                0,
                height,
                width,
                buf,
                win_x,
                win_y,
                cols,
                rows,
                clip,
            );
            return;
        }
        let rows_per_band = height.div_ceil(workers as u32);
        let band_stride = Self::pixel_index(width as usize, 0, rows_per_band as usize);
        std::thread::scope(|s| {
            for (band_idx, chunk) in self.back.chunks_mut(band_stride).enumerate() {
                let by0 = (band_idx as u32).saturating_mul(rows_per_band);
                let band_rows = Self::rows_in(chunk, width);
                s.spawn(move || {
                    Self::blit_opaque_band(
                        chunk, by0, band_rows, width, buf, win_x, win_y, cols, rows, clip,
                    );
                });
            }
        });
    }

    /// Copy the opaque source rows that land inside one framebuffer row-band.
    ///
    /// `band` holds `band_rows` contiguous scanlines of `fb_width` pixels each,
    /// the first at absolute framebuffer row `by0`. For each source row
    /// `r in 0..rows`, the destination row is `win_y + r`; rows outside
    /// `[by0, by0 + band_rows)` belong to another band and are skipped, so calling
    /// this over a full row-band partition of the framebuffer reproduces the
    /// serial per-row [`copy_row`](Self::copy_row) blit exactly. Horizontal
    /// clipping (left `src_off` when `win_x < 0`, right-edge `min`) mirrors
    /// `copy_row` byte-for-byte.
    #[allow(clippy::too_many_arguments)]
    fn blit_opaque_band(
        band: &mut [u32],
        by0: u32,
        band_rows: u32,
        fb_width: u32,
        buf: &SharedBuffer,
        win_x: i32,
        win_y: i32,
        cols: u32,
        rows: u32,
        clip: Option<Rect>,
    ) {
        let width_usize = fb_width as usize;
        let band_end = by0.saturating_add(band_rows);
        // Resolve the occlusion clip to half-open pixel bounds once, rather than
        // per row: it is the same rectangle for every row of the blit.
        let clip_bounds = clip.map(|c| {
            (
                c.x.max(0) as u32,
                c.x.saturating_add(c.width as i32).max(0) as u32,
                c.y.max(0) as u32,
                c.y.saturating_add(c.height as i32).max(0) as u32,
            )
        });
        for r in 0..rows {
            let sy = win_y.saturating_add(r as i32);
            if sy < 0 {
                continue;
            }
            let sy = sy as u32;
            // Rows past the framebuffer bottom (band_end == fb_height for the last
            // band) and rows owned by other bands are both handled by this range.
            if sy < by0 || sy >= band_end {
                continue;
            }
            let Some(full_src) = buf.row(r) else {
                continue;
            };
            // Matches blit_buffer: clamp the source run to `cols`, but never grow
            // it past the actual row length.
            let src = full_src.get(..cols as usize).unwrap_or(full_src);
            if src.is_empty() {
                continue;
            }
            // Replicate copy_row's clipping exactly for byte-identical output.
            let (src_off, dst_x) = if win_x < 0 {
                let skip = win_x.unsigned_abs() as usize;
                if skip >= src.len() {
                    continue;
                }
                (skip, 0usize)
            } else {
                (0usize, win_x as usize)
            };
            if dst_x >= width_usize {
                continue;
            }
            // Both subtractions are guarded above, so saturating is exact.
            let count = src
                .len()
                .saturating_sub(src_off)
                .min(width_usize.saturating_sub(dst_x));
            if count == 0 {
                continue;
            }
            // Same narrowing as `copy_row`: trim the destination to the clip and
            // walk the source forward by the amount the left edge moved.
            let (src_off, dst_x, count) = match clip_bounds {
                None => (src_off, dst_x, count),
                Some((cx0, cx1, cy0, cy1)) => {
                    if sy < cy0 || sy >= cy1 {
                        continue;
                    }
                    let lo = (dst_x as u32).max(cx0);
                    let dst_end = u32::try_from(dst_x.saturating_add(count)).unwrap_or(u32::MAX);
                    let hi = dst_end.min(cx1);
                    if hi <= lo {
                        continue;
                    }
                    (
                        src_off.saturating_add((lo as usize).saturating_sub(dst_x)),
                        lo as usize,
                        hi.saturating_sub(lo) as usize,
                    )
                }
            };
            // `sy >= by0` was established above, so the row is band-local.
            let band_row = sy.saturating_sub(by0) as usize;
            let dst_span =
                Self::row_range(width_usize, band_row, dst_x, dst_x.saturating_add(count));
            if let (Some(dst), Some(s)) = (
                band.get_mut(dst_span),
                src.get(src_off..src_off.saturating_add(count)),
            ) {
                dst.copy_from_slice(s);
            }
        }
    }

    /// Fill a horizontal span of back-buffer row `y`, columns `[x_start, x_end)`,
    /// with a fully-opaque `color` using a single slice `fill()`.
    ///
    /// This is the fast path for solid (alpha 255) rectangle fills: it skips the
    /// per-pixel alpha math and bounds check that `blend_pixel` performs. The
    /// caller must have already clipped `x_start`/`x_end`/`y` to the framebuffer.
    ///
    /// OPT: replaces a per-pixel `blend_pixel` loop with one `[u32]::fill`, which
    /// the compiler lowers to a `memset`-style store. This is the dominant win in
    /// the 4K compositor benchmark (BENCH-COMPOSITOR-SLOW): opaque `FillRect`s
    /// (window backgrounds, decorations) no longer pay per-pixel float alpha cost.
    #[inline]
    fn fill_row_solid(&mut self, y: u32, x_start: u32, x_end: u32, color: u32) {
        let Some((x_lo, x_hi)) = self.clip_span(y, x_start, x_end) else {
            return;
        };
        let row = Self::row_range(
            self.width as usize,
            y as usize,
            x_lo as usize,
            x_hi as usize,
        );
        if let Some(span) = self.back.get_mut(row) {
            span.fill(color | 0xFF_00_00_00);
        }
    }

    /// Blend a horizontal span of back-buffer row `y`, columns `[x_start, x_end)`,
    /// with `src_color` at pre-computed integer alpha `src_a` (0..=255).
    ///
    /// OPT: hoists the alpha computation and per-pixel branch/float conversion out
    /// of the inner loop (versus calling `blend_pixel` per pixel). Only the integer
    /// channel blend runs per pixel. Caller guarantees `0 < src_a < 255`.
    ///
    /// The `src * a` products used to be hoisted by hand; they are now inside
    /// [`blend_channel`](Self::blend_channel), which is a `#[inline] const fn`
    /// of two loop-invariant arguments, so LLVM hoists them itself. Taking
    /// `src_a` as a `u8` also puts half the caller's contract into the type.
    #[inline]
    fn blend_row(&mut self, y: u32, x_start: u32, x_end: u32, src_color: u32, src_a: u8) {
        let Some((x_lo, x_hi)) = self.clip_span(y, x_start, x_end) else {
            return;
        };
        let (src_r, src_g, src_b) = (
            (src_color >> 16) as u8,
            (src_color >> 8) as u8,
            src_color as u8,
        );
        let row = Self::row_range(
            self.width as usize,
            y as usize,
            x_lo as usize,
            x_hi as usize,
        );
        if let Some(span) = self.back.get_mut(row) {
            for pixel in span {
                let dst = *pixel;
                let out_r = Self::blend_channel(src_r, (dst >> 16) as u8, src_a);
                let out_g = Self::blend_channel(src_g, (dst >> 8) as u8, src_a);
                let out_b = Self::blend_channel(src_b, dst as u8, src_a);
                *pixel = 0xFF_00_00_00 | (out_r << 16) | (out_g << 8) | out_b;
            }
        }
    }

    /// Get a reference to the front buffer for display.
    pub fn front_buffer(&self) -> &[u32] {
        &self.front
    }

    /// Resize the framebuffer. Clears all contents.
    pub fn resize(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidDimensions { width, height });
        }
        if width > MAX_FB_WIDTH || height > MAX_FB_HEIGHT {
            return Err(CompositorError::FramebufferTooLarge { width, height });
        }

        // Bounded by the MAX_FB_* check just above; saturating so the bound is
        // enforced by the code rather than only by the reader.
        let size = (width as usize).saturating_mul(height as usize);
        self.width = width;
        self.height = height;
        self.back = vec![0xFF_00_00_00; size];
        self.front = vec![0xFF_00_00_00; size];
        Ok(())
    }

    /// The inclusive pixel bounds a line may land on: the framebuffer, narrowed
    /// by the caller's clip rectangle. Deliberately *not* narrowed by the
    /// framebuffer's own `frame_clip` — `blend_pixel` applies that per pixel,
    /// and being conservative here only costs a few iterations that were being
    /// spent anyway.
    fn line_bounds(&self, clip: Option<&Rect>) -> (i64, i64, i64, i64) {
        let (mut x_lo, mut y_lo) = (0i64, 0i64);
        let mut x_hi = i64::from(self.width).saturating_sub(1);
        let mut y_hi = i64::from(self.height).saturating_sub(1);
        if let Some(c) = clip {
            x_lo = x_lo.max(i64::from(c.x));
            y_lo = y_lo.max(i64::from(c.y));
            x_hi = x_hi.min(
                i64::from(c.x)
                    .saturating_add(i64::from(c.width))
                    .saturating_sub(1),
            );
            y_hi = y_hi.min(
                i64::from(c.y)
                    .saturating_add(i64::from(c.height))
                    .saturating_sub(1),
            );
        }
        (x_lo, x_hi, y_lo, y_hi)
    }

    /// Blend one line pixel, honouring the draw clip. Coordinates are `i64`
    /// because the caller works in that width; anything outside `u32` is off
    /// every framebuffer and is dropped here.
    fn plot_line_pixel(&mut self, x: i64, y: i64, clip: Option<&Rect>, c: u32, o: f32) {
        let (Ok(px), Ok(py)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if clip.is_some_and(|r| !r.contains(x as i32, y as i32)) {
            return;
        }
        self.blend_pixel(px, py, c, o);
    }
}

// ---------------------------------------------------------------------------
// Software rendering backend
// ---------------------------------------------------------------------------

/// The CPU rasterizer: [`Framebuffer`] as a [`RenderTarget`].
///
/// This is the implementation the compositor has always had, restated against
/// the seam rather than rewritten — every fast path survives intact
/// (parallel row-band clears, the opaque-blit memcpy, per-row solid fills, the
/// clipped Bresenham walk). What changed is only that the callers no longer
/// name it.
///
/// Several method names shadow inherent methods of the same name and meaning
/// (`clear`, `clear_rect`, `clear_except`, `resize`). Inherent methods win
/// method-call resolution, so an existing `fb.clear(c)` still reaches the
/// inherent one and the two can never disagree — they are the same code. The
/// bodies below use fully-qualified `Framebuffer::` paths so that is visible
/// rather than merely true.
impl RenderTarget for Framebuffer {
    #[inline]
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn resize(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        Framebuffer::resize(self, width, height)
    }

    #[inline]
    fn set_frame_clip(&mut self, clip: Option<Rect>) {
        Framebuffer::set_frame_clip(self, clip);
    }

    fn clear(&mut self, color: u32) {
        Framebuffer::clear(self, color);
    }

    fn clear_rect(&mut self, rect: &Rect, color: u32) {
        Framebuffer::clear_rect(self, rect, color);
    }

    fn clear_except(&mut self, color: u32, covered: &[Rect]) {
        Framebuffer::clear_except(self, color, covered);
    }

    /// `rect` arrives already intersected with the client's clip stack, so all
    /// that is left is to resolve it against the surface and pick a per-row
    /// path.
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): opaque fills become a single slice memset
    /// per row; translucent fills hoist the alpha math out of the inner loop,
    /// instead of blending pixel by pixel.
    fn fill_rect(&mut self, rect: Rect, color: u32, opacity: f32) {
        let x_start = rect.x.max(0) as u32;
        let y_start = rect.y.max(0) as u32;
        let x_end = rect.right().max(0) as u32;
        let y_end = rect.bottom().max(0) as u32;
        if x_end <= x_start || y_end <= y_start {
            return;
        }

        // Resolved once (colour alpha scaled by window opacity) rather than per
        // pixel; a fill that is entirely transparent is not a fill.
        let src_a = Self::effective_alpha(color, opacity);
        if src_a == 0 {
            return;
        }
        if src_a == 255 {
            for row in y_start..y_end {
                self.fill_row_solid(row, x_start, x_end, color);
            }
        } else {
            for row in y_start..y_end {
                self.blend_row(row, x_start, x_end, color, src_a);
            }
        }
    }

    /// Bresenham, clipped along its major axis.
    ///
    /// The endpoints arrive from a client's [`RenderCommand::Line`] and are not
    /// the compositor's to trust. The original loop stepped one pixel at a time
    /// from `(x1, y1)` all the way to `(x2, y2)` no matter where the screen
    /// was, so a line spanning the coordinate space cost four *billion*
    /// iterations of a display-server thread — a hang any client could ask for
    /// — and it computed `x2 - x1`, `.abs()` and `2 * err` in `i32`, each of
    /// which overflows on that same input (`(-2^31).abs()` panics outright).
    ///
    /// So the major-axis step range is intersected with the drawable area up
    /// front, in `i64`, and only the surviving steps are walked — at most one
    /// per framebuffer column or row. Every pixel the old code could actually
    /// have made visible is still visited, in the same order and colour; the
    /// dropped steps are exactly those whose `blend_pixel` was already a no-op.
    /// `line_matches_the_unclipped_bresenham_walk` pins that equivalence
    /// against a transcription of the old loop.
    ///
    /// The minor axis keeps Bresenham's incremental form, but is *seeded* at
    /// the first surviving step from the closed form
    /// `round(k · minor / major)` — computed once in `u128`, since `k · minor`
    /// can reach 2^64 — so skipping to the visible part costs one division
    /// rather than one iteration per skipped pixel.
    fn draw_line(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    ) {
        let (x1, y1, x2, y2) = (i64::from(x1), i64::from(y1), i64::from(x2), i64::from(y2));

        let (adx, ady) = (x2.abs_diff(x1), y2.abs_diff(y1));
        let steps = adx.max(ady);
        if steps == 0 {
            self.plot_line_pixel(x1, y1, clip, color, opacity);
            return;
        }
        let sx: i64 = if x2 >= x1 { 1 } else { -1 };
        let sy: i64 = if y2 >= y1 { 1 } else { -1 };

        // Walk whichever axis moves faster; the other is derived from it.
        let x_major = adx >= ady;
        let (bx_lo, bx_hi, by_lo, by_hi) = self.line_bounds(clip);
        let (major_0, major_dir, minor_0, minor_dir, b_lo, b_hi) = if x_major {
            (x1, sx, y1, sy, bx_lo, bx_hi)
        } else {
            (y1, sy, x1, sx, by_lo, by_hi)
        };

        // Steps whose major coordinate `major_0 + major_dir·k` lands in bounds.
        // All four operands are within i32 range, so these differences are far
        // inside i64 and the saturating forms are exact.
        let (raw_lo, raw_hi) = if major_dir > 0 {
            (b_lo.saturating_sub(major_0), b_hi.saturating_sub(major_0))
        } else {
            (major_0.saturating_sub(b_hi), major_0.saturating_sub(b_lo))
        };
        if raw_hi < 0 {
            return;
        }
        let k_lo = raw_lo.max(0) as u64;
        let k_hi = (raw_hi as u64).min(steps);
        if k_lo > k_hi {
            return;
        }

        // minor(k) = floor((2·k·minor_len + major_len) / (2·major_len)), i.e.
        // round-half-up of k·minor_len/major_len, which is what the error
        // accumulator in the classic loop computes.
        let (minor_len, major_len) = if x_major { (ady, adx) } else { (adx, ady) };
        let two_major = u128::from(major_len).saturating_mul(2);
        let num = u128::from(k_lo)
            .saturating_mul(2)
            .saturating_mul(u128::from(minor_len))
            .saturating_add(u128::from(major_len));
        // `major_len == steps >= 1` here, so `two_major` is never zero; the
        // fallbacks are unreachable and exist only because the divisor's
        // nonzero-ness is an argument rather than a type.
        let mut q = num.checked_div(two_major).unwrap_or(0) as u64;
        let mut rem = num.checked_rem(two_major).unwrap_or(0) as u64;
        // `2·minor_len <= 2·major_len`, so a step can push `rem` past
        // `two_major` at most once — no inner loop is needed.
        let (step, wrap) = (minor_len.saturating_mul(2), two_major as u64);

        for k in k_lo..=k_hi {
            let major = major_0.saturating_add(major_dir.saturating_mul(k as i64));
            let minor = minor_0.saturating_add(minor_dir.saturating_mul(q as i64));
            let (px, py) = if x_major {
                (major, minor)
            } else {
                (minor, major)
            };
            self.plot_line_pixel(px, py, clip, color, opacity);

            rem = rem.saturating_add(step);
            if rem >= wrap {
                rem = rem.saturating_sub(wrap);
                q = q.saturating_add(1);
            }
        }
    }

    /// Blend one glyph's coverage into the back buffer.
    ///
    /// Coverage scales the window's opacity rather than being blended by
    /// `osfont` itself: the compositor's pixels go through a clip stack and a
    /// window opacity that `osfont::Target` does not model, so it asks for the
    /// coverage values and does its own blending — a half-covered pixel of a
    /// half-transparent window is a quarter opaque, which is what multiplying
    /// the two gives.
    fn draw_glyph(
        &mut self,
        mask: &GlyphMask,
        pen: f32,
        baseline: f32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    ) {
        // The origin comes from another process's layout, so it may be anything
        // at all; a non-finite one is dropped rather than cast, because `as`
        // turns NaN into 0 and would stamp the glyph at the top-left of the
        // screen.
        let (ox, oy) = (pen + mask.left as f32, baseline + mask.top as f32);
        if !ox.is_finite() || !oy.is_finite() {
            return;
        }
        let (ox, oy) = (ox.round() as i32, oy.round() as i32);

        for row in 0..mask.height {
            let fy = oy.saturating_add(row as i32);
            if fy < 0 {
                continue;
            }
            for col in 0..mask.width {
                let coverage = mask.at(col, row);
                if coverage == 0 {
                    continue;
                }
                let fx = ox.saturating_add(col as i32);
                if fx < 0 {
                    continue;
                }
                if let Some(clip_rect) = clip
                    && !clip_rect.contains(fx, fy)
                {
                    continue;
                }
                // `blend_pixel` clamps and discards anything outside the
                // framebuffer.
                let alpha = opacity * (coverage as f32 / 255.0);
                self.blend_pixel(fx as u32, fy as u32, color, alpha);
            }
        }
    }

    /// OPT: when the buffer is opaque (Xrgb) and the window is fully opaque, the
    /// per-row content is copied straight into the framebuffer
    /// ([`copy_row`](Framebuffer::copy_row)) instead of running a per-pixel
    /// float-alpha blend — O(h) row memcpys vs O(w·h) blends. This is the common
    /// game/video case and the path runs every frame for non-fullscreen
    /// buffer-backed windows (fullscreen ones bypass blitting via direct
    /// scanout). The opaque copy is bit-identical to the blend result because
    /// `blend_pixel` writes `src | 0xFF000000` for opaque pixels and imported
    /// Xrgb pixels already carry 0xFF alpha.
    fn blit_buffer(
        &mut self,
        buf: &SharedBuffer,
        x: i32,
        y: i32,
        cols: u32,
        rows: u32,
        opacity: f32,
    ) {
        if opacity >= 1.0 && buf.is_opaque() {
            // Per-row-independent opaque copies — parallelized across row bands.
            self.blit_opaque(buf, x, y, cols, rows);
            return;
        }
        for row in 0..rows {
            let sy = y.saturating_add(row as i32);
            if sy < 0 {
                continue;
            }
            for col in 0..cols {
                let sx = x.saturating_add(col as i32);
                if sx < 0 {
                    continue;
                }
                if let Some(px) = buf.pixel(col, row) {
                    self.blend_pixel(sx as u32, sy as u32, px, opacity);
                }
            }
        }
    }

    fn present(&mut self) {
        self.swap();
    }

    #[inline]
    fn presented_pixels(&self) -> &[u32] {
        &self.front
    }

    #[inline]
    fn working_pixels(&self) -> &[u32] {
        &self.back
    }
}

// ---------------------------------------------------------------------------
// Display management
// ---------------------------------------------------------------------------

/// A physical or virtual display.
#[derive(Clone, Debug)]
pub struct Display {
    /// Display identifier.
    pub id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// DPI scale factor (1.0 = 96dpi, 2.0 = 192dpi).
    pub scale_factor: f32,
    /// Offset in the virtual display space (for multi-monitor).
    pub offset_x: i32,
    /// Offset in the virtual display space (for multi-monitor).
    pub offset_y: i32,
    /// Whether this is the primary display.
    pub primary: bool,
}

impl Display {
    /// Create a new display with the given parameters.
    pub fn new(
        id: u32,
        width: u32,
        height: u32,
        refresh_rate: u32,
        scale_factor: f32,
        primary: bool,
    ) -> Self {
        Self {
            id,
            width,
            height,
            refresh_rate,
            scale_factor,
            offset_x: 0,
            offset_y: 0,
            primary,
        }
    }

    /// Get the frame interval for this display's refresh rate.
    pub const fn frame_interval(&self) -> Duration {
        frame_interval_for(self.refresh_rate)
    }

    /// Get the display's bounding rectangle in virtual space.
    pub fn bounds(&self) -> Rect {
        Rect::new(self.offset_x, self.offset_y, self.width, self.height)
    }
}

/// Multi-monitor display layout.
pub struct DisplayManager {
    /// All connected displays.
    displays: Vec<Display>,
}

impl DisplayManager {
    /// Create a display manager with a single primary display.
    pub fn new(width: u32, height: u32, refresh_rate: u32) -> Self {
        Self {
            displays: vec![Display::new(0, width, height, refresh_rate, 1.0, true)],
        }
    }

    /// Get the primary display.
    pub fn primary(&self) -> Option<&Display> {
        self.displays.iter().find(|d| d.primary)
    }

    /// Add an additional display to the layout.
    pub fn add_display(&mut self, mut display: Display) {
        // Place it to the right of the existing displays by default.
        if self.displays.is_empty() {
            display.primary = true;
        } else {
            let rightmost = self
                .displays
                .iter()
                .map(|d| d.bounds().right())
                .max()
                .unwrap_or(0);
            display.offset_x = rightmost;
        }
        self.displays.push(display);
    }

    /// Remove the display with `id`, answering it, or `None` if no display has
    /// that id.
    ///
    /// **The survivors keep the offsets they already had.** Removing the
    /// leftmost of two monitors therefore leaves the other one at its old `x`,
    /// so the virtual desktop starts part-way along and the space the departed
    /// screen occupied stays in the bounding box as a hole that is composited
    /// and scanned out nowhere. Re-flowing them leftwards is the tidier
    /// arrangement and is deliberately not done: the *scanout* does not re-flow
    /// its surviving heads when one dies (design-decisions.md §515), because a
    /// head that fails mid-session must not drag the other monitors' pictures
    /// sideways, and these two layouts have to agree pixel for pixel or every
    /// window is drawn on the wrong screen. One of them has to be the authority
    /// and it is the one holding the framebuffers.
    ///
    /// If the display removed was the primary, the first survivor is promoted.
    /// Nothing tolerates a primary-less arrangement: [`Self::primary`] is the
    /// fallback for every "which monitor is this?" question, and an arrangement
    /// with monitors but no primary answers `None` to all of them.
    ///
    /// Prefer [`Compositor::detach_display`], which also resizes the scanout
    /// surface and re-places the windows the departed monitor was holding.
    pub fn remove_display(&mut self, id: u32) -> Option<Display> {
        let index = self.displays.iter().position(|d| d.id == id)?;
        let removed = self.displays.remove(index);
        if removed.primary {
            if let Some(first) = self.displays.first_mut() {
                first.primary = true;
            }
        }
        Some(removed)
    }

    /// Get the total virtual desktop bounds (union of all displays).
    pub fn virtual_bounds(&self) -> Rect {
        self.bounds_folded(|d| d.bounds())
    }

    /// The virtual desktop bounds that *would* result from the display at
    /// `index` being `width` by `height`.
    ///
    /// Asked before the change is made rather than after, because the scanout
    /// surface has to be reallocated to match and that allocation can fail. A
    /// compositor that had already adopted the new arrangement when it found
    /// out would be describing a desktop it cannot paint; asking first means a
    /// failed mode change leaves everything exactly as it was.
    ///
    /// An `index` that names no display answers the current bounds, which is
    /// the truthful answer to "what would change?" — nothing.
    pub fn virtual_bounds_if_resized(&self, index: usize, width: u32, height: u32) -> Rect {
        self.bounds_folded(|d| {
            if self.displays.get(index).is_some_and(|t| t.id == d.id) {
                Rect::new(d.offset_x, d.offset_y, width, height)
            } else {
                d.bounds()
            }
        })
    }

    /// The union of every display's rectangle, each mapped through `f` first.
    fn bounds_folded(&self, f: impl Fn(&Display) -> Rect) -> Rect {
        // Taken from the iterator rather than by `[0]` after an `is_empty()`
        // guard: the guard and the index are two statements that have to agree
        // about the same fact, and only one of them is checked by the compiler.
        let mut rest = self.displays.iter();
        let Some(first) = rest.next() else {
            return Rect::new(0, 0, 0, 0);
        };
        rest.fold(f(first), |bounds, display| bounds.union(&f(display)))
    }

    /// Get all displays.
    pub fn displays(&self) -> &[Display] {
        &self.displays
    }

    /// The display a window belongs to: the one it overlaps most.
    ///
    /// Largest-intersection rather than "the display containing the top-left
    /// corner", because the corner rule gives the wrong answer for exactly the
    /// window a user would ask about — one dragged mostly onto the second
    /// monitor still has its top-left on the first, and would keep the first
    /// monitor's scaling while nine tenths of it sat on the second.
    ///
    /// A window overlapping nothing — dragged into a gap between monitors, or
    /// off the virtual desktop entirely — answers the primary display rather
    /// than `None`. Callers want a scale factor, and there is no sensible "no
    /// scale"; falling back to the primary keeps such a window drawn at the
    /// size it had before it was dragged off-screen.
    pub fn display_for(&self, rect: &Rect) -> Option<&Display> {
        self.displays
            .iter()
            .filter_map(|d| {
                // `u64` because the product of two virtual-desktop dimensions
                // need not fit `u32` even though each dimension does. It
                // cannot overflow `u64` either, but a saturating multiply says
                // so in a form the compiler checks, and costs the same.
                rect.intersect(&d.bounds())
                    .map(|i| (u64::from(i.width).saturating_mul(u64::from(i.height)), d))
            })
            .filter(|&(area, _)| area > 0)
            // `max_by_key` returns the *last* maximum, so an exact tie — a
            // window centred on the seam — goes to the rightmost display.
            // Either answer is defensible; what matters is that it is
            // deterministic rather than dependent on hotplug order.
            .max_by_key(|&(area, _)| area)
            .map(|(_, d)| d)
            .or_else(|| self.primary())
    }

    /// The scale factor to draw a window's decorations at.
    ///
    /// Separate from [`display_for`](Self::display_for) so the common caller
    /// does not have to handle an `Option` it has no answer for: with no
    /// displays connected at all, 1.0 is the only scale that leaves geometry
    /// unchanged rather than collapsing it.
    pub fn scale_for(&self, rect: &Rect) -> f32 {
        self.display_for(rect).map_or(1.0, |d| d.scale_factor)
    }

    /// Get the refresh rate of the primary display.
    pub fn primary_refresh_rate(&self) -> u32 {
        self.primary().map_or(60, |d| d.refresh_rate)
    }
}

// ---------------------------------------------------------------------------
// Frame timing / stats
// ---------------------------------------------------------------------------

/// Frame timing and performance statistics.
#[derive(Clone, Debug)]
pub struct FrameStats {
    /// Time taken to composite the last frame (microseconds).
    pub last_frame_time_us: u64,
    /// Total frames composited since startup.
    pub frames_composited: u64,
    /// Frames dropped (compose took longer than frame interval).
    pub dropped_frames: u64,
    /// Frames presented via fullscreen direct-scanout bypass (no compositing).
    pub bypass_frames: u64,
    /// Target frame interval based on display refresh rate.
    pub target_interval: Duration,
    /// Timestamp of the last frame start.
    last_frame_start: Option<Instant>,
}

impl FrameStats {
    /// Create new frame stats with the given target interval.
    pub fn new(target_interval: Duration) -> Self {
        Self {
            last_frame_time_us: 0,
            frames_composited: 0,
            dropped_frames: 0,
            bypass_frames: 0,
            target_interval,
            last_frame_start: None,
        }
    }

    /// Mark the start of a new frame.
    pub fn begin_frame(&mut self) {
        self.last_frame_start = Some(Instant::now());
    }

    /// Mark the end of a frame. Returns true if the frame was within budget.
    pub fn end_frame(&mut self) -> bool {
        let elapsed = self
            .last_frame_start
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);

        self.last_frame_time_us = elapsed.as_micros() as u64;
        self.frames_composited = self.frames_composited.saturating_add(1);

        if elapsed > self.target_interval {
            self.dropped_frames = self.dropped_frames.saturating_add(1);
            false
        } else {
            true
        }
    }

    /// Check if enough time has passed since the last frame to start a new one.
    pub fn should_compose(&self) -> bool {
        match self.last_frame_start {
            Some(start) => start.elapsed() >= self.target_interval,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Compositor protocol (stub IPC)
// ---------------------------------------------------------------------------

/// Requests from clients to the compositor.
#[derive(Clone, Debug)]
pub enum CompositorRequest {
    /// Create a new window on the terms the client asked for.
    ///
    /// Carries the whole [`WindowSpec`] rather than title/width/height,
    /// because anything it does not carry is something the compositor cannot
    /// honour: before it did, every window was decorated, resizable and opaque
    /// no matter what the client wanted. `client_pid` is separate because it is
    /// not part of the request — the compositor knows which connection the
    /// request arrived on and the client does not get to claim otherwise.
    CreateWindow { spec: WindowSpec, client_pid: u64 },
    /// Destroy an existing window.
    DestroyWindow { window_id: WindowId },
    /// Set the window title.
    SetTitle { window_id: WindowId, title: String },
    /// Submit render commands for a window's client area.
    Submit {
        window_id: WindowId,
        commands: Vec<RenderCommand>,
    },
    /// Move a window to a new position.
    Move { window_id: WindowId, x: i32, y: i32 },
    /// Resize a window's client area.
    Resize {
        window_id: WindowId,
        width: u32,
        height: u32,
    },
    /// Minimize a window.
    Minimize { window_id: WindowId },
    /// Maximize a window.
    Maximize { window_id: WindowId },
    /// Enter or leave fullscreen (enables direct-scanout bypass for games).
    SetFullscreen { window_id: WindowId, enable: bool },
    /// Restore a window from minimized/maximized state.
    Restore { window_id: WindowId },
    /// Show or hide a window without destroying it.
    SetVisible { window_id: WindowId, visible: bool },
    /// Set the cursor shape for a window.
    SetCursor {
        window_id: WindowId,
        cursor: CursorShape,
    },
    /// Set window opacity.
    SetOpacity { window_id: WindowId, opacity: f32 },
    /// Query display information.
    GetDisplayInfo,
    /// Re-read the user's `appearance.yaml` and adopt whatever it now says.
    ///
    /// Carries no settings: see
    /// [`guiremote::control::RequestBody::ReloadAppearance`] for why a
    /// notification rather than a setter, and [`Compositor::reload_appearance`]
    /// for what it does.
    ReloadAppearance,
    /// Re-read the user's `input.yaml` and adopt whatever it now says.
    ///
    /// Carries no settings, for the reason given on
    /// [`guiremote::control::RequestBody::ReloadInput`]; see
    /// [`Compositor::reload_input`] for what it does and what it deliberately
    /// leaves alone.
    ReloadInput,
    /// Begin a remote draw-command stream session (returns a stream id).
    StreamStart,
    /// Capture the current scene for a stream session as an encoded wire frame.
    StreamCapture { stream_id: u64 },
    /// End a remote draw-command stream session.
    StreamStop { stream_id: u64 },
    /// Act on a window on a shell's behalf, rather than on its owner's.
    ///
    /// Every other window request here arrives having been checked against the
    /// windows the sending connection owns (`ClientLink::resolve`). This one
    /// cannot be — a taskbar button exists to act on somebody else's window —
    /// so it is a separate variant precisely so that the exception is visible
    /// at the point the compositor decides what to do, rather than hidden
    /// inside a shared verb.
    ShellControl {
        window_id: WindowId,
        action: ShellControlAction,
    },
    /// Reserve a strip along an edge of the monitor a panel window is on, so
    /// that tiling stops short of it. Answered with
    /// [`CompositorResponse::WorkArea`].
    ///
    /// Named against the *panel's own* window, so unlike
    /// [`ShellControl`](Self::ShellControl) it is resolved against the sender's
    /// windows in the ordinary way. It is nonetheless privileged, because its
    /// effect lands on everybody else's tiling rather than on the window it
    /// names — see [`guiremote::control::RequestBody::ReserveEdge`].
    ReserveEdge {
        window_id: WindowId,
        edge: PanelEdge,
        size: u32,
    },
    /// Show a different virtual desktop. Answered with
    /// [`CompositorResponse::Ok`].
    ///
    /// Names no window -- which is why it could not be a
    /// [`ShellControlAction`], every one of which is a verb aimed at one. It is
    /// privileged for the plainest possible reason: it changes what every
    /// client on the machine is showing.
    SwitchWorkspace { workspace: u32 },
    /// File a window on a virtual desktop. Answered with
    /// [`CompositorResponse::Ok`].
    ///
    /// Like [`ShellControl`](Self::ShellControl) it names somebody else's
    /// window and is therefore not resolved against the sender's own.
    SetWindowWorkspace { window_id: WindowId, workspace: u32 },
}

/// Responses from the compositor to clients.
#[derive(Clone, Debug)]
pub enum CompositorResponse {
    /// A window was created successfully.
    WindowCreated { window_id: WindowId },
    /// Operation completed successfully.
    Ok,
    /// Operation failed.
    Error { message: String },
    /// Display information response.
    DisplayInfo {
        width: u32,
        height: u32,
        refresh_rate: u32,
        scale_factor: f32,
    },
    /// A remote stream session was started.
    StreamStarted { stream_id: u64 },
    /// An encoded draw-command stream frame (see [`stream`] wire format).
    StreamFrame { data: Vec<u8> },
    /// The usable rectangle left on a monitor after every reservation on it,
    /// in whole pixels and in virtual-desktop coordinates. Answer to
    /// [`CompositorRequest::ReserveEdge`].
    WorkArea {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

/// Notifications sent from the compositor to clients (events).
#[derive(Clone, Debug)]
pub enum EventNotification {
    /// Keyboard event for the focused window.
    KeyEvent {
        window_id: WindowId,
        /// Scan-code-set-1 code, extended keys carrying their `0xE0` prefix in
        /// the high byte. Forwarded to clients alongside the resolved key name
        /// so that games and remappers can read physical key positions
        /// (`design-decisions.md` §456).
        scancode: u32,
        /// The key this scancode means under the system keymap.
        key: Key,
        pressed: bool,
        /// Modifiers held *at the moment this key changed state*, including
        /// this key itself if it is a modifier.
        modifiers: Modifiers,
        /// The text this keystroke produced — see [`ClientKeyEvent::text`],
        /// whose shape this is and into which it is copied unchanged.
        ///
        /// A `String` rather than the [`CompositorInput::KeyDown`] side's
        /// `Option<char>` because the two ends of this translation are not
        /// symmetric: a keystroke *arrives* carrying at most one character —
        /// that is all a scancode plus a level can name — but *leaves*
        /// carrying however many the layout made of it, which is none for a
        /// dead key and two when a composition fails.
        text: String,
    },
    /// Mouse event for a window.
    MouseEvent {
        window_id: WindowId,
        x: i32,
        y: i32,
        kind: MouseEventKind,
    },
    /// Window close was requested (close button clicked).
    WindowClose { window_id: WindowId },
    /// Window was resized by the user.
    WindowResized {
        window_id: WindowId,
        width: u32,
        height: u32,
    },
    /// Window gained keyboard focus.
    FocusGained { window_id: WindowId },
    /// Window lost keyboard focus.
    FocusLost { window_id: WindowId },
}

impl EventNotification {
    /// The window this notification is addressed to.
    ///
    /// Every notification has one — an event with no addressee would be an
    /// event nobody can be told about — and the wire front end needs it to
    /// decide which client's connection the event goes down. Without that,
    /// every client would be sent every other client's keystrokes.
    pub const fn window_id(&self) -> WindowId {
        match self {
            Self::KeyEvent { window_id, .. }
            | Self::MouseEvent { window_id, .. }
            | Self::WindowClose { window_id }
            | Self::WindowResized { window_id, .. }
            | Self::FocusGained { window_id }
            | Self::FocusLost { window_id } => *window_id,
        }
    }
}

/// Translate one compositor notification into the wire event a client receives.
///
/// The two vocabularies differ deliberately rather than accidentally. The
/// compositor's side is about hardware and windows — a scancode, a window id,
/// integer screen-derived coordinates. The client's side
/// ([`guitk::event::Event`]) is what widget code is written against: a named
/// key, float coordinates in the widget's own space, and no window id at all,
/// because a client already knows which of its windows it is dispatching to.
/// This function is the single place that crossing happens, so the mapping is
/// stated once instead of being re-improvised per app.
fn wire_event(n: EventNotification) -> guiremote::InputEvent {
    match n {
        EventNotification::KeyEvent {
            window_id,
            scancode,
            key,
            pressed,
            modifiers,
            text,
        } => guiremote::InputEvent::key(
            window_id.0,
            ClientKeyEvent {
                key,
                pressed,
                modifiers,
                text,
            },
            scancode,
        ),
        EventNotification::MouseEvent {
            window_id,
            x,
            y,
            kind,
        } => guiremote::InputEvent::new(
            window_id.0,
            ClientEvent::Mouse(ClientMouseEvent {
                // Widened, not converted: these are already window-local
                // integers, and every value an i32 can hold is exactly
                // representable in f32 only up to 2^24. A coordinate that
                // large is not a real window, so the lossy cast is bounded by
                // the display's size rather than by the type.
                x: x as f32,
                y: y as f32,
                kind: wire_mouse_kind(kind),
            }),
        ),
        EventNotification::WindowClose { window_id } => {
            guiremote::InputEvent::new(window_id.0, ClientEvent::CloseRequested)
        }
        EventNotification::WindowResized {
            window_id,
            width,
            height,
        } => guiremote::InputEvent::new(window_id.0, ClientEvent::Resize { width, height }),
        EventNotification::FocusGained { window_id } => {
            guiremote::InputEvent::new(window_id.0, ClientEvent::FocusIn)
        }
        EventNotification::FocusLost { window_id } => {
            guiremote::InputEvent::new(window_id.0, ClientEvent::FocusOut)
        }
    }
}

/// The client-side mouse kind for a compositor one.
///
/// `Enter`/`Leave` and `DoubleClick` exist only on the client side and are
/// never produced here: entering a *widget* is the client's business, since the
/// compositor does not know where a client's widgets are, and double-click
/// timing belongs with the widget that has to honour it. Synthesising them here
/// would be guessing at layout the compositor cannot see.
const fn wire_mouse_kind(kind: MouseEventKind) -> ClientMouseKind {
    match kind {
        MouseEventKind::Move => ClientMouseKind::Move,
        MouseEventKind::ButtonPress(b) => ClientMouseKind::Press(wire_button(b)),
        MouseEventKind::ButtonRelease(b) => ClientMouseKind::Release(wire_button(b)),
        MouseEventKind::Scroll { dx, dy } => ClientMouseKind::Scroll { dx, dy },
    }
}

const fn wire_button(b: MouseButton) -> ClientMouseButton {
    match b {
        MouseButton::Left => ClientMouseButton::Left,
        MouseButton::Right => ClientMouseButton::Right,
        MouseButton::Middle => ClientMouseButton::Middle,
        MouseButton::Back => ClientMouseButton::Back,
        MouseButton::Forward => ClientMouseButton::Forward,
    }
}

/// Mouse event kind for notifications.
#[derive(Clone, Copy, Debug)]
pub enum MouseEventKind {
    Move,
    ButtonPress(MouseButton),
    ButtonRelease(MouseButton),
    Scroll { dx: f32, dy: f32 },
}

// ---------------------------------------------------------------------------
// Drag state
// ---------------------------------------------------------------------------

/// What kind of drag operation is in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    /// Dragging the window by its title bar.
    MoveWindow,
    /// Resizing from the left edge.
    ResizeLeft,
    /// Resizing from the right edge.
    ResizeRight,
    /// Resizing from the top edge.
    ResizeTop,
    /// Resizing from the bottom edge.
    ResizeBottom,
    /// Resizing from the top-left corner.
    ResizeTopLeft,
    /// Resizing from the top-right corner.
    ResizeTopRight,
    /// Resizing from the bottom-left corner.
    ResizeBottomLeft,
    /// Resizing from the bottom-right corner.
    ResizeBottomRight,
}

impl DragMode {
    /// Which edge this drag moves on each axis, as `(horizontal, vertical)`.
    ///
    /// Splitting the nine modes into two independent axes is what lets one
    /// piece of arithmetic serve all of them. The eight resize arms it replaces
    /// each spelled the same sum out again, which is why a single overflow
    /// appeared in eight places at once, and why two of them had drifted into
    /// using the literals `100`/`50` where `MIN_WINDOW_WIDTH`/`_HEIGHT` were
    /// meant.
    const fn resize_edges(self) -> (Edge, Edge) {
        match self {
            // Not a resize; the caller handles it before asking.
            Self::MoveWindow => (Edge::Fixed, Edge::Fixed),
            Self::ResizeLeft => (Edge::Near, Edge::Fixed),
            Self::ResizeRight => (Edge::Far, Edge::Fixed),
            Self::ResizeTop => (Edge::Fixed, Edge::Near),
            Self::ResizeBottom => (Edge::Fixed, Edge::Far),
            Self::ResizeTopLeft => (Edge::Near, Edge::Near),
            Self::ResizeTopRight => (Edge::Far, Edge::Near),
            Self::ResizeBottomLeft => (Edge::Near, Edge::Far),
            Self::ResizeBottomRight => (Edge::Far, Edge::Far),
        }
    }
}

/// How one axis of a window responds to a resize drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    /// This axis is not being dragged: extent and origin both stay put.
    Fixed,
    /// The far edge follows the pointer, so only the extent changes.
    Far,
    /// The near edge follows the pointer, so the origin moves too — by exactly
    /// the amount the extent changed, which is not the same as the amount the
    /// pointer moved.
    Near,
}

impl Edge {
    /// The extent this drag asks for, before the window's own limits apply.
    ///
    /// Computed in `i64` because that is the only width in which the sum is
    /// total: a `u32` extent plus an `i32` delta ranges wider than either type
    /// holds, so every 32-bit spelling of this must either overflow or pre-clamp
    /// away the very case it is clamping. The pointer position that produces
    /// `delta` is not the compositor's to bound — it arrives from a device, or
    /// from a client injecting one.
    fn extent(self, start: u32, delta: i32) -> u32 {
        let delta = match self {
            Self::Fixed => return start,
            Self::Far => i64::from(delta),
            Self::Near => i64::from(delta).saturating_neg(),
        };
        i64::from(start)
            .saturating_add(delta)
            .clamp(0, i64::from(u32::MAX)) as u32
    }

    /// Where the origin ends up, given the extent the window *settled* on.
    ///
    /// Taking the settled extent rather than the requested one is what keeps a
    /// window from drifting out from under the pointer: a client with a
    /// `min_size` larger than the drag asked for gets its minimum, and the near
    /// edge has to move by that difference, not by the one the drag wanted.
    /// Only the window knows which it got, so only it can be asked.
    fn origin(self, start_origin: i32, start_extent: u32, settled: u32) -> i32 {
        match self {
            Self::Fixed | Self::Far => start_origin,
            Self::Near => {
                let shift = i64::from(start_extent).saturating_sub(i64::from(settled));
                start_origin
                    .saturating_add(shift.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
            }
        }
    }
}

/// Active drag state.
#[derive(Clone, Debug)]
struct DragState {
    /// The window being dragged.
    window_id: WindowId,
    /// The kind of drag.
    mode: DragMode,
    /// Mouse position at drag start.
    start_mouse: Point,
    /// Window position at drag start.
    start_window_pos: Point,
    /// Window size at drag start.
    start_window_size: (u32, u32),
}

// ---------------------------------------------------------------------------
// Clip and translation stacks (for rendering)
// ---------------------------------------------------------------------------

/// Clipping rectangle stack for the rendering engine.
#[derive(Clone, Debug, Default)]
struct ClipStack {
    stack: Vec<Rect>,
}

impl ClipStack {
    fn push(&mut self, rect: Rect) {
        // Intersect with current clip (if any) to narrow the visible area.
        let effective = if let Some(current) = self.stack.last() {
            current.intersect(&rect).unwrap_or(Rect::new(0, 0, 0, 0))
        } else {
            rect
        };
        self.stack.push(effective);
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn current(&self) -> Option<&Rect> {
        self.stack.last()
    }

    fn clear(&mut self) {
        self.stack.clear();
    }
}

/// Translation offset stack for the rendering engine.
#[derive(Clone, Debug, Default)]
struct TranslateStack {
    stack: Vec<(f32, f32)>,
    /// Cumulative offset (sum of all pushed translations).
    total_dx: f32,
    total_dy: f32,
}

impl TranslateStack {
    fn push(&mut self, dx: f32, dy: f32) {
        self.stack.push((dx, dy));
        self.total_dx += dx;
        self.total_dy += dy;
    }

    fn pop(&mut self) {
        if let Some((dx, dy)) = self.stack.pop() {
            self.total_dx -= dx;
            self.total_dy -= dy;
        }
    }

    fn offset(&self) -> (f32, f32) {
        (self.total_dx, self.total_dy)
    }

    fn clear(&mut self) {
        self.stack.clear();
        self.total_dx = 0.0;
        self.total_dy = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

/// Translates the toolkit's weight hint into the one `osfont` understands.
///
/// `Light` maps to regular because the built-in face has two weights and no
/// third; drawing it bold would be worse than drawing it regular.
fn weight_of(hint: FontWeightHint) -> Weight {
    match hint {
        FontWeightHint::Bold => Weight::Bold,
        FontWeightHint::Regular | FontWeightHint::Light => Weight::Regular,
    }
}

/// Translates the toolkit's family into the one `osfont` understands.
fn family_of(family: FontFamily) -> Family {
    match family {
        FontFamily::Ui => Family::Ui,
        FontFamily::Mono => Family::Mono,
    }
}

/// Draw a shaped run from `pen`, stopping before the first glyph that would
/// cross `limit`, and leave `pen` where it stopped.
///
/// Extracted so the run and its ellipsis are drawn by the same code: the mark
/// is a shaped run like any other, and a second hand-written glyph loop for it
/// would be a second place for the baseline, the clip and the mark offsets to
/// drift out of agreement with the first.
///
/// Free rather than a method for the same borrow reason as [`blend_mask`], and
/// takes the font by `&mut` because the glyph cache rasterises on demand.
///
/// `spans` colours the run per glyph, by the byte each glyph came from; empty —
/// which it is for every plain `Text` command — draws the whole run in `color`.
/// Resolving colour *here*, inside the one walk that already visits every
/// glyph, is what lets a multi-coloured string be one shaped run: the
/// alternative is to cut the string per colour and shape the pieces, which is
/// the bug this parameter exists to fix. See `RenderCommand::RichText`.
#[allow(clippy::too_many_arguments)]
fn blit_run<T: RenderTarget + ?Sized>(
    fb: &mut T,
    font: &mut osfont::system::SystemFont,
    run: &osfont::shape::ShapedRun,
    pen: &mut f32,
    baseline: f32,
    limit: Option<f32>,
    color: u32,
    spans: &[TextSpan],
    opacity: f32,
    clip: Option<&Rect>,
) {
    // Drawing order, not logical order: the two differ only when the run
    // contains right-to-left text, and there the logical order would put the
    // first letter of a Hebrew word on the left — the word backwards. The
    // truncation below is a further reason: `limit` has to cut the glyphs that
    // fall off the *right of the line*, which is what this walk reaches last
    // only in this order.
    for shaped in run.draw_order() {
        let advance = shaped.advance;
        // Measured before drawing, so a glyph that would cross the limit is
        // dropped whole rather than clipped down the middle.
        if let Some(mx) = limit
            && *pen + advance > mx
        {
            break;
        }
        // Resolved before the mask is fetched: `glyph_mask` borrows the font
        // mutably for as long as the mask lives, and this needs nothing from it.
        let ink = TextSpan::color_at(spans, shaped.cluster).map_or(color, |c| color_to_argb(&c));
        if let Some(mask) = font.glyph_mask(shaped.key) {
            // `offset` is zero except on an attached combining mark, and its
            // `y` points up where the screen's points down.
            //
            // The mask goes to the backend rather than being blended here:
            // shaping is backend-independent (a GPU backend uploads these same
            // masks to a glyph atlas), so the split falls exactly between
            // "which coverage, where" and "how it reaches the surface".
            fb.draw_glyph(
                mask,
                *pen + shaped.offset.0,
                baseline - shaped.offset.1,
                ink,
                opacity,
                clip,
            );
        }
        *pen += advance;
    }
}

// ---------------------------------------------------------------------------
// Rendering engine
// ---------------------------------------------------------------------------

/// The rendering engine rasterizes RenderCommands to the framebuffer.
struct RenderEngine {
    clip_stack: ClipStack,
    translate_stack: TranslateStack,
    /// Same type, same rounding rule, and — because the faces are installed by
    /// [`guitk::text::install_ui_faces`] rather than chosen here — the same
    /// files as the toolkit measured with. A label the toolkit sized therefore
    /// cannot be drawn in a different font than the one it was measured in.
    ///
    /// A cache of its own, rather than the toolkit's process-global one,
    /// because this process only ever draws: it would take that cache's lock
    /// for every glyph run and never once measure anything.
    fonts: FontCache,
    /// The families pushed by [`RenderCommand::PushFont`], innermost last.
    ///
    /// A stack rather than a single value so the scopes nest the way the clip
    /// and translate ones do: a terminal pane inside a UI window pushes `Mono`,
    /// and the status bar drawn after it must get the UI face back rather than
    /// whatever the last push happened to be.
    font_stack: Vec<FontFamily>,
}

impl RenderEngine {
    fn new() -> Self {
        let mut fonts = FontCache::new();
        match guitk::text::install_ui_faces(&mut fonts) {
            Some(family) => eprintln!("compositor: UI font: {family}"),
            // Not fatal: the cache keeps its built-in bitmap face, so text
            // still draws. Worth saying out loud, though, since every label on
            // the screen will look wrong and the cause is off-screen.
            None => eprintln!(
                "compositor: no system UI font found, falling back to the built-in bitmap face"
            ),
        }
        // Resolved here for the same reason as the UI face: the process that
        // measured a terminal's grid picked its family from this same list, so
        // resolving it the same way is what keeps the cells the app laid out
        // and the glyphs drawn into them the same width.
        match guitk::text::install_mono_faces(&mut fonts) {
            Some(family) => eprintln!("compositor: monospace font: {family}"),
            None => eprintln!(
                "compositor: no monospace font found, falling back to the built-in bitmap face"
            ),
        }
        Self {
            clip_stack: ClipStack::default(),
            translate_stack: TranslateStack::default(),
            fonts,
            font_stack: Vec::new(),
        }
    }

    /// Execute a list of render commands, drawing into the framebuffer within
    /// the given window region.
    fn execute<T: RenderTarget + ?Sized>(
        &mut self,
        fb: &mut T,
        commands: &[RenderCommand],
        window_x: i32,
        window_y: i32,
        window_width: u32,
        window_height: u32,
        opacity: f32,
    ) {
        // Set up initial clip to the window's client area.
        self.clip_stack.clear();
        self.translate_stack.clear();
        self.font_stack.clear();
        self.clip_stack
            .push(Rect::new(window_x, window_y, window_width, window_height));
        // Push the window origin as the base translation.
        self.translate_stack.push(window_x as f32, window_y as f32);

        for cmd in commands {
            self.execute_command(fb, cmd, opacity);
        }

        self.clip_stack.clear();
        self.translate_stack.clear();
        // Cleared rather than asserted empty: the command list came from
        // another process, so an unbalanced push is that process's bug and
        // must not leak into the next window this engine draws.
        self.font_stack.clear();
    }

    fn execute_command<T: RenderTarget + ?Sized>(
        &mut self,
        fb: &mut T,
        cmd: &RenderCommand,
        opacity: f32,
    ) {
        let (tx, ty) = self.translate_stack.offset();

        match cmd {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                self.fill_round_rect(
                    fb,
                    px,
                    py,
                    w,
                    h,
                    corner_radii,
                    color_to_argb(color),
                    opacity,
                );
            }
            RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                color,
                line_width,
                corner_radii,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                let lw = (*line_width).max(1.0) as u32;
                self.stroke_round_rect(
                    fb,
                    px,
                    py,
                    w,
                    h,
                    lw,
                    corner_radii,
                    color_to_argb(color),
                    opacity,
                );
            }
            RenderCommand::Text {
                x,
                y,
                text,
                color,
                font_size,
                font_weight,
                max_width,
                overflow,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let max_w = max_width.map(|w| w as u32);
                self.draw_text(
                    fb,
                    px,
                    py,
                    text,
                    color_to_argb(color),
                    &[],
                    opacity,
                    max_w,
                    *font_size,
                    *font_weight,
                    *overflow,
                );
            }
            RenderCommand::RichText {
                x,
                y,
                text,
                spans,
                color,
                font_size,
                font_weight,
                max_width,
                overflow,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let max_w = max_width.map(|w| w as u32);
                self.draw_text(
                    fb,
                    px,
                    py,
                    text,
                    color_to_argb(color),
                    spans,
                    opacity,
                    max_w,
                    *font_size,
                    *font_weight,
                    *overflow,
                );
            }
            RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width: _,
            } => {
                let px1 = (*x1 + tx) as i32;
                let py1 = (*y1 + ty) as i32;
                let px2 = (*x2 + tx) as i32;
                let py2 = (*y2 + ty) as i32;
                self.draw_line(fb, px1, py1, px2, py2, color_to_argb(color), opacity);
            }
            RenderCommand::PushClip {
                x,
                y,
                width,
                height,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                self.clip_stack
                    .push(Rect::new(px, py, *width as u32, *height as u32));
            }
            RenderCommand::PopClip => {
                self.clip_stack.pop();
            }
            RenderCommand::PushTranslate { dx, dy } => {
                self.translate_stack.push(*dx, *dy);
            }
            RenderCommand::PopTranslate => {
                self.translate_stack.pop();
            }
            RenderCommand::PushFont { family } => {
                self.font_stack.push(*family);
            }
            RenderCommand::PopFont => {
                self.font_stack.pop();
            }
            RenderCommand::Image { .. } => {
                // Image rendering requires an asset store — stub for now.
            }
            RenderCommand::BoxShadow {
                x,
                y,
                width,
                height,
                offset_x,
                offset_y,
                blur,
                spread,
                color,
                corner_radii,
            } => {
                // Simplified shadow: draw a semi-transparent rectangle expanded by spread+blur.
                //
                // `try_from`, not `as`: a negative spread — a shadow the caller
                // wants drawn *inside* its box, which CSS permits — makes the
                // expanded extent negative, and `as u32` would reinterpret that
                // as roughly four billion pixels and fill the whole screen. A
                // shadow smaller than nothing is nothing.
                let expand = (*spread + *blur) as i32;
                let grow = expand.saturating_mul(2);
                let px = ((*x + tx + *offset_x) as i32).saturating_sub(expand);
                let py = ((*y + ty + *offset_y) as i32).saturating_sub(expand);
                let w = u32::try_from((*width as i32).saturating_add(grow)).unwrap_or(0);
                let h = u32::try_from((*height as i32).saturating_add(grow)).unwrap_or(0);
                // The shadow grows with the box, and so does its rounding: a
                // square shadow under a rounded window shows its own corners
                // sticking out past the curve they are supposed to sit behind.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "an expansion in pixels; f32 is exact well past any of them"
                )]
                let grown = expand as f32;
                let radii = CornerRadii {
                    top_left: (corner_radii.top_left + grown).max(0.0),
                    top_right: (corner_radii.top_right + grown).max(0.0),
                    bottom_right: (corner_radii.bottom_right + grown).max(0.0),
                    bottom_left: (corner_radii.bottom_left + grown).max(0.0),
                };
                self.fill_round_rect(fb, px, py, w, h, &radii, color_to_argb(color), opacity);
            }
        }
    }

    /// Resolve a rectangle against the clip stack and hand it to the backend.
    ///
    /// The clip intersection happens here rather than below the seam because
    /// for an axis-aligned quad the intersection *is* the clip — resolving it
    /// in the caller lets a fully-clipped fill be dropped without the backend
    /// ever hearing about it, and spares every backend from re-implementing the
    /// clip stack.
    fn fill_rect<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u32,
        opacity: f32,
    ) {
        let draw_rect = Rect::new(x, y, width, height);
        if let Some(clipped) = self.effective_clip(&draw_rect) {
            fb.fill_rect(clipped, color, opacity);
        }
    }

    /// Stroke (outline) a rectangle.
    fn stroke_rect<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        line_width: u32,
        color: u32,
        opacity: f32,
    ) {
        let rect = Rect::new(x, y, width, height);
        let line = i32::try_from(line_width).unwrap_or(i32::MAX);
        // Top edge
        self.fill_rect(fb, x, y, width, line_width, color, opacity);
        // Bottom edge
        self.fill_rect(
            fb,
            x,
            rect.bottom().saturating_sub(line),
            width,
            line_width,
            color,
            opacity,
        );
        // Left edge
        self.fill_rect(fb, x, y, line_width, height, color, opacity);
        // Right edge
        self.fill_rect(
            fb,
            rect.right().saturating_sub(line),
            y,
            line_width,
            height,
            color,
            opacity,
        );
    }

    /// Fill a rectangle whose corners are rounded by `radii`.
    ///
    /// Square corners fall through to [`RenderEngine::fill_rect`] untouched, so
    /// the overwhelmingly common case is bit-identical to what was drawn before
    /// rounding existed and costs nothing to have gained it. That fall-through
    /// is also what keeps this affordable: the rounded path emits one draw per
    /// scanline *of the corner bands only*, and the flat middle — almost all of
    /// a window — stays a single quad.
    fn fill_round_rect<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        radii: &CornerRadii,
        color: u32,
        opacity: f32,
    ) {
        let Some(shape) = RoundRect::new(x, y, width, height, radii) else {
            self.fill_rect(fb, x, y, width, height, color, opacity);
            return;
        };

        for row in shape.top_rows() {
            if let Some((left, right)) = shape.span(row) {
                self.fill_span_row(fb, row, left, right, color, opacity);
            }
        }

        let middle = shape.middle_rows();
        let middle_height = u32::try_from(middle.end.saturating_sub(middle.start)).unwrap_or(0);
        if middle_height > 0 {
            self.fill_rect(fb, x, middle.start, width, middle_height, color, opacity);
        }

        for row in shape.bottom_rows() {
            if let Some((left, right)) = shape.span(row) {
                self.fill_span_row(fb, row, left, right, color, opacity);
            }
        }
    }

    /// Outline a rectangle whose corners are rounded by `radii`.
    ///
    /// The whole outline is "the outer shape minus the inner shape", evaluated
    /// one scanline at a time, which is why it reuses [`RoundRect::span`]
    /// rather than growing arc code of its own: an outline whose curve is
    /// computed differently from the fill it surrounds is an outline that
    /// misses its own fill by a pixel somewhere.
    fn stroke_round_rect<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        line_width: u32,
        radii: &CornerRadii,
        color: u32,
        opacity: f32,
    ) {
        let Some(outer) = RoundRect::new(x, y, width, height, radii) else {
            self.stroke_rect(fb, x, y, width, height, line_width, color, opacity);
            return;
        };
        let inner = outer.inset_by(line_width);

        // The two straight side bars, coalesced. Between the corner bands both
        // the outer and the inner edge are vertical, so these rows need no
        // per-scanline work — and for a tall window they are nearly every row.
        let middle = outer.middle_rows();
        let middle_height = u32::try_from(middle.end.saturating_sub(middle.start)).unwrap_or(0);
        if middle_height > 0 {
            if line_width.saturating_mul(2) >= width {
                // The bars meet or overlap: one solid run, because drawing two
                // overlapping ones would blend the overlap twice and leave a
                // darker stripe down the middle of a translucent border.
                self.fill_rect(fb, x, middle.start, width, middle_height, color, opacity);
            } else {
                let right_bar = x.saturating_add_unsigned(width.saturating_sub(line_width));
                self.fill_rect(
                    fb,
                    x,
                    middle.start,
                    line_width,
                    middle_height,
                    color,
                    opacity,
                );
                self.fill_rect(
                    fb,
                    right_bar,
                    middle.start,
                    line_width,
                    middle_height,
                    color,
                    opacity,
                );
            }
        }

        for row in outer.top_rows().chain(outer.bottom_rows()) {
            let Some((outer_left, outer_right)) = outer.span(row) else {
                continue;
            };
            match inner.as_ref().and_then(|shape| shape.span(row)) {
                // Two arcs of the ring on this scanline, one per side.
                Some((inner_left, inner_right)) => {
                    self.fill_span_row(fb, row, outer_left, inner_left, color, opacity);
                    self.fill_span_row(fb, row, inner_right, outer_right, color, opacity);
                }
                // Above or below the inner shape entirely — the cap of the
                // ring, which is solid across.
                None => self.fill_span_row(fb, row, outer_left, outer_right, color, opacity),
            }
        }
    }

    /// Paint `[left, right)` of one scanline, feathering the pixel at each end
    /// by how much of it the span actually covers.
    ///
    /// **The single place antialiasing happens.** Both rounded primitives above
    /// reduce to spans and route through here, so a curve cannot be smooth in a
    /// fill and jagged in the outline drawn over it.
    ///
    /// Coverage rides in on `opacity` rather than on a separate channel because
    /// that is exactly what it means for a solid colour: a pixel half-covered
    /// by an opaque fill and a pixel fully covered by a half-transparent one
    /// composite identically, and [`RenderTarget::fill_rect`] already blends by
    /// opacity. A one-pixel-wide fill is therefore an antialiased edge, and
    /// every backend gets it without a new trait method to implement.
    ///
    /// Only horizontal coverage is measured. Where the arc runs steeply — the
    /// middle of each quadrant — that is very nearly exact; where it runs flat,
    /// at the extreme top and bottom of a corner, it understates the smoothing.
    /// The alternative is per-pixel area sampling over the corner boxes, which
    /// costs `radius²` blends per corner instead of two, and at the radii this
    /// codebase actually uses (4, 8 and 16 px, from `WindowCorners`) the
    /// difference is not visible.
    fn fill_span_row<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        row_y: i32,
        left: f32,
        right: f32,
        color: u32,
        opacity: f32,
    ) {
        if right <= left {
            return;
        }
        // `as i32` on `f32` saturates rather than wrapping, so a nonsense
        // coordinate from a client becomes a far-offscreen one that the clip
        // discards — not a wrapped one that lands back on screen.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "saturating float-to-int cast; out-of-range coordinates are \
                      clipped away rather than wrapped"
        )]
        let to_px = |v: f32| v as i32;

        let first = left.floor();
        // Entirely inside a single pixel: one blend at its partial coverage,
        // not a zero-width solid run flanked by two.
        if right <= first + 1.0 {
            self.fill_rect(
                fb,
                to_px(first),
                row_y,
                1,
                1,
                color,
                opacity * (right - left),
            );
            return;
        }

        let solid_start = left.ceil();
        let solid_end = right.floor();

        let left_coverage = solid_start - left;
        if left_coverage > 0.0 {
            self.fill_rect(
                fb,
                to_px(first),
                row_y,
                1,
                1,
                color,
                opacity * left_coverage,
            );
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "`solid_end >= solid_start` holds by construction and both \
                      are bounded by the primitive's own i32 extent"
        )]
        let solid_width = (solid_end - solid_start) as u32;
        if solid_width > 0 {
            self.fill_rect(
                fb,
                to_px(solid_start),
                row_y,
                solid_width,
                1,
                color,
                opacity,
            );
        }
        let right_coverage = right - solid_end;
        if right_coverage > 0.0 {
            self.fill_rect(
                fb,
                to_px(solid_end),
                row_y,
                1,
                1,
                color,
                opacity * right_coverage,
            );
        }
    }

    /// Draw a run of text with its **top-left** at `(x, y)`.
    ///
    /// Top-left rather than the baseline `osfont` works in, because that is
    /// what every caller here already passes and what the toolkit's layout
    /// produces; the ascent is added once, below, instead of at each call
    /// site.
    ///
    /// Glyph coverage is folded into the per-window opacity rather than blended
    /// by `osfont` itself. The compositor's pixels go through a clip stack and
    /// a window opacity that `osfont::Target` does not model, so it asks for
    /// the coverage values and does its own blending — a half-covered pixel of
    /// a half-transparent window is a quarter opaque, which is exactly what
    /// multiplying the two gives.
    #[allow(clippy::too_many_arguments)]
    fn draw_text<T: RenderTarget + ?Sized>(
        &mut self,
        fb: &mut T,
        x: i32,
        y: i32,
        text: &str,
        color: u32,
        spans: &[TextSpan],
        opacity: f32,
        max_width: Option<u32>,
        size: f32,
        weight: FontWeightHint,
        overflow: TextOverflow,
    ) {
        let clip = self.clip_stack.current().copied();
        let family = self.font_stack.last().copied().unwrap_or_default();
        let font = self.fonts.get(size, weight_of(weight), family_of(family));
        let baseline = y as f32 + font.metrics().ascent;
        let max_x = max_width.map(|w| x.saturating_add(w as i32));

        // Shaped rather than walked character by character, so this run is
        // spaced exactly as the toolkit measured it — same kerning, same
        // ligatures, same tab width. Laying text out here by a different rule
        // from the process that sized the widget is how every centred label
        // ends up off by half the difference, with neither process looking
        // wrong on its own.
        let run = font.shape(text);

        // Decide about the ellipsis *before* drawing anything, because it
        // changes where the real glyphs have to stop: the mark has to fit
        // inside `max_width` too, or it is just a differently-shaped overflow.
        //
        // Three conditions, and all three have to hold: the caller asked for
        // the mark; the run actually overruns, so a string that fits is never
        // decorated with an ellipsis it did not earn; and the mark itself fits
        // inside `max_width`, since a mark that overruns is just a
        // differently-shaped overflow.
        //
        // Note what is deliberately *not* a condition: that a real glyph still
        // fits afterwards. In a field with room for the mark and nothing else
        // we draw the bare `…`, which says "there is a value here you cannot
        // read" — true — in preference to one lone letter, which says "the
        // value is M" — false. Being honest about what was lost is the whole
        // point of the field; showing one more character is not.
        let mut limit = max_x.map(|mx| mx as f32);
        let mut ellipsis = None;
        if overflow == TextOverflow::Ellipsis
            && let Some(mx) = max_x
        {
            let width: f32 = run.draw_order().map(|g| g.advance).sum();
            if x as f32 + width > mx as f32 {
                let mark = font.shape("…");
                let mark_width: f32 = mark.draw_order().map(|g| g.advance).sum();
                // Subtracted as f32, not as i32: `x` and `mx` both arrive from
                // another process, and `mx - x` is in range only by an argument
                // about how `saturating_add` bounds it. An argument is not a
                // guard, and this one costs nothing to make unnecessary.
                if mark_width <= mx as f32 - x as f32 {
                    limit = Some(mx as f32 - mark_width);
                    ellipsis = Some(mark);
                }
            }
        }

        let mut pen = x as f32;
        blit_run(
            fb,
            font,
            &run,
            &mut pen,
            baseline,
            limit,
            color,
            spans,
            opacity,
            clip.as_ref(),
        );
        if let Some(mark) = ellipsis {
            // Unbounded on purpose: the room was reserved above, so bounding it
            // again could only round it away and put us back where we started —
            // a cut with nothing to show for it.
            //
            // Uncoloured by the spans, too: the mark stands for the text that
            // was *not* drawn, so it belongs to no byte and no token. Colouring
            // it as if it were byte zero — which is what passing `spans` here
            // would do, the mark being its own run with its own clusters — would
            // paint the "there is more" mark in the colour of the first word.
            blit_run(
                fb,
                font,
                &mark,
                &mut pen,
                baseline,
                None,
                color,
                &[],
                opacity,
                clip.as_ref(),
            );
        }
    }

    /// Resolve the client's clip stack and hand the line to the backend.
    ///
    /// The whole walk lives below the seam rather than here: a line is a
    /// primitive a GPU draws directly, so a caller that rasterized it into
    /// per-pixel blends would be handing the backend the one shape it did not
    /// need help with. What stays here is the part that is not rasterization —
    /// which clip rectangle is in force.
    fn draw_line<T: RenderTarget + ?Sized>(
        &self,
        fb: &mut T,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        opacity: f32,
    ) {
        let clip = self.clip_stack.current().copied();
        fb.draw_line(x1, y1, x2, y2, color, opacity, clip.as_ref());
    }

    /// Compute the effective clip rectangle by intersecting the draw area
    /// with the current clip stack.
    fn effective_clip(&self, draw_rect: &Rect) -> Option<Rect> {
        match self.clip_stack.current() {
            Some(clip) => clip.intersect(draw_rect),
            None => Some(*draw_rect),
        }
    }
}

// ---------------------------------------------------------------------------
// Rounded rectangles
// ---------------------------------------------------------------------------

/// A rectangle with per-corner rounding, resolved once into the form the
/// scanline loops want.
///
/// Constructing this is where every *question* about the shape is settled —
/// are the radii finite, do they overlap, is the rounding even visible, which
/// rows contain an arc — so that [`RoundRect::span`], which runs once per
/// scanline, is pure arithmetic with no branches on validity. The radii come
/// from a client over the wire and the client is under no obligation to send
/// sane ones.
#[derive(Clone, Copy)]
struct RoundRect {
    /// Continuous coordinates of the edges: `left`/`top` inclusive,
    /// `right`/`bottom` exclusive, matching [`Rect`]'s own convention.
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    top_left: f32,
    top_right: f32,
    bottom_right: f32,
    bottom_left: f32,
    /// First scanline of the shape, and the row past its last.
    first_row: i32,
    end_row: i32,
    /// The row past the top corner band, and the first row of the bottom one.
    /// Between them every scanline is the full width and can be one quad.
    top_band_end: i32,
    bottom_band_start: i32,
}

impl RoundRect {
    /// Resolve a rounding request, or `None` if it does not describe a curve.
    ///
    /// `None` is the signal to take the flat path, and it deliberately covers
    /// more than `CornerRadii::ZERO`: a sub-half-pixel radius cannot move a
    /// single pixel, so treating it as square is not an approximation, it is
    /// the same image for less work. Degenerate and hostile inputs — zero
    /// extent, infinities, NaN — land here too rather than reaching the square
    /// roots, where a NaN would poison every comparison it took part in and
    /// silently erase the shape.
    fn new(x: i32, y: i32, width: u32, height: u32, radii: &CornerRadii) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let sane = |r: f32| if r.is_finite() && r > 0.0 { r } else { 0.0 };
        let mut top_left = sane(radii.top_left);
        let mut top_right = sane(radii.top_right);
        let mut bottom_right = sane(radii.bottom_right);
        let mut bottom_left = sane(radii.bottom_left);
        if top_left.max(top_right).max(bottom_right).max(bottom_left) < 0.5 {
            return None;
        }

        #[allow(
            clippy::cast_precision_loss,
            reason = "extents are screen-sized; f32 is exact well past any of them"
        )]
        let (w, h) = (width as f32, height as f32);

        // CSS's overlap rule (CSS Backgrounds 3 §5.5): two radii sharing a side
        // may not together exceed it, and when any pair does, *every* radius is
        // scaled by the same worst-case factor. Scaling only the offending pair
        // would round one corner of a small box and leave its neighbour square,
        // which looks like a bug rather than like a clamp.
        let shrink = [
            (w, top_left + top_right),
            (w, bottom_left + bottom_right),
            (h, top_left + bottom_left),
            (h, top_right + bottom_right),
        ]
        .into_iter()
        .filter(|&(_, sum)| sum > 0.0)
        .map(|(side, sum)| side / sum)
        .fold(1.0_f32, f32::min);
        if shrink < 1.0 {
            top_left *= shrink;
            top_right *= shrink;
            bottom_right *= shrink;
            bottom_left *= shrink;
        }

        let end_row = y.saturating_add_unsigned(height);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "radii are clamped to the extent above, so each ceiling is \
                      at most `height`, which is a `u32`"
        )]
        let (top_band, bottom_band) = (
            top_left.max(top_right).ceil() as u32,
            bottom_left.max(bottom_right).ceil() as u32,
        );
        // Clamping the *pair* is not implied by the clamp above: radii on
        // opposite corners of a diagonal (top-left and bottom-right, say) each
        // satisfy the side rule at a full `height` yet together span twice it.
        // The bands must not cross, or the middle quad would have negative
        // height and the two loops would paint the same rows.
        let top_band = top_band.min(height);
        let bottom_band = bottom_band.min(height.saturating_sub(top_band));

        #[allow(
            clippy::cast_precision_loss,
            reason = "screen coordinates; f32 is exact well past any of them"
        )]
        let (left, top) = (x as f32, y as f32);

        Some(Self {
            left,
            top,
            right: left + w,
            bottom: top + h,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            first_row: y,
            end_row,
            top_band_end: y.saturating_add_unsigned(top_band),
            bottom_band_start: end_row.saturating_sub_unsigned(bottom_band),
        })
    }

    /// Scanlines containing the top corners' arcs.
    fn top_rows(&self) -> core::ops::Range<i32> {
        self.first_row..self.top_band_end
    }

    /// Scanlines containing the bottom corners' arcs.
    fn bottom_rows(&self) -> core::ops::Range<i32> {
        self.bottom_band_start..self.end_row
    }

    /// Scanlines where the shape is the full width — one quad, not a loop.
    fn middle_rows(&self) -> core::ops::Range<i32> {
        self.top_band_end..self.bottom_band_start
    }

    /// The shape's horizontal extent on the scanline `row_y`, or `None` if the
    /// scanline misses it.
    ///
    /// Sampled at the pixel's *centre*, which is what makes the coverage the
    /// caller derives from these edges symmetric: sampling at the top of the
    /// pixel would bias every arc half a pixel upwards, and the bias would be
    /// visible as a seam where a rounded fill meets the outline around it.
    fn span(&self, row_y: i32) -> Option<(f32, f32)> {
        #[allow(
            clippy::cast_precision_loss,
            reason = "screen coordinates; f32 is exact well past any of them"
        )]
        let centre = row_y as f32 + 0.5;
        if centre < self.top || centre >= self.bottom {
            return None;
        }
        let left = self.left + self.inset(centre, self.top_left, self.bottom_left);
        let right = self.right - self.inset(centre, self.top_right, self.bottom_right);
        if right <= left {
            None
        } else {
            Some((left, right))
        }
    }

    /// How far in from one vertical edge that edge's outline lies, on the
    /// scanline whose centre is `centre`.
    ///
    /// Zero everywhere between the two arcs, which is most of a window and is
    /// why the middle band needs no per-row work at all.
    fn inset(&self, centre: f32, top_radius: f32, bottom_radius: f32) -> f32 {
        // Each arc is a quarter of a circle whose centre sits one radius in
        // from the corner along both axes; `dy` is the distance from that
        // centre's scanline.
        let (dy, radius) = if centre < self.top + top_radius {
            (self.top + top_radius - centre, top_radius)
        } else if centre > self.bottom - bottom_radius {
            (centre - (self.bottom - bottom_radius), bottom_radius)
        } else {
            return 0.0;
        };
        // `max(0.0)` guards the square root against the rounding that can make
        // `r² - dy²` a very small negative at the extreme tip of an arc, where
        // `dy` and `r` are equal to within one ulp.
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    }

    /// The shape `distance` pixels inside this one — the hole an outline of
    /// that width leaves — or `None` when the outline is thick enough to close
    /// it entirely and the "outline" is really a fill.
    fn inset_by(&self, distance: u32) -> Option<Self> {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a line width in pixels; f32 is exact well past any of them"
        )]
        let d = distance as f32;
        let left = self.left + d;
        let top = self.top + d;
        let right = self.right - d;
        let bottom = self.bottom - d;
        if right <= left || bottom <= top {
            return None;
        }
        Some(Self {
            left,
            top,
            right,
            bottom,
            // A corner tighter than the outline is thick has no hole left in
            // it: the inner shape simply squares off there, which is what a
            // real rounded border does.
            top_left: (self.top_left - d).max(0.0),
            top_right: (self.top_right - d).max(0.0),
            bottom_right: (self.bottom_right - d).max(0.0),
            bottom_left: (self.bottom_left - d).max(0.0),
            // Never iterated — `stroke_round_rect` walks the *outer* shape's
            // rows and only asks this one for spans — but kept consistent so a
            // future caller that does iterate it is not surprised.
            first_row: self.first_row.saturating_add_unsigned(distance),
            end_row: self.end_row.saturating_sub_unsigned(distance),
            top_band_end: self.top_band_end,
            bottom_band_start: self.bottom_band_start,
        })
    }
}

/// Convert a guitk Color to ARGB u32 format.
#[inline]
fn color_to_argb(color: &Color) -> u32 {
    (color.a as u32) << 24 | (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

// ---------------------------------------------------------------------------
// Theme colors for window decorations
// ---------------------------------------------------------------------------

/// Colors used for window decoration rendering, in the framebuffer's own ARGB.
///
/// This is [`appearance::DecorationColors`] with the channel packing already
/// done. It is a separate type rather than the settings' own because the
/// conversion is per-colour arithmetic and the alternative is doing it at every
/// blit: a frame draws a title bar, a border and three buttons per window, and
/// the colours only change when the user changes them.
///
/// Which is also why there is no constructor that invents a palette. The
/// twelve hardcoded constants that used to live here were a fourth opinion
/// about what a title bar looks like, and the visible symptom was that a user
/// in light mode got a dark-navy desktop and a dark blue-gray title bar from
/// the process that actually draws them.
struct DecorationTheme {
    /// Title bar background when focused.
    title_bar_focused: u32,
    /// Title bar background when unfocused.
    title_bar_unfocused: u32,
    /// Title text color when focused.
    title_text_focused: u32,
    /// Title text color when unfocused.
    title_text_unfocused: u32,
    /// Close button color.
    close_button: u32,
    /// Maximize button color.
    maximize_button: u32,
    /// Minimize button color.
    minimize_button: u32,
    /// Window border color when focused.
    border_focused: u32,
    /// Window border color when unfocused.
    border_unfocused: u32,
    /// Shadow color (semi-transparent black).
    shadow_color: u32,
    /// Desktop background color.
    desktop_background: u32,
}

impl DecorationTheme {
    /// Resolve the frame colours from the user's settings, packed for the
    /// framebuffer.
    fn from_settings(settings: &AppearanceSettings) -> Self {
        let colors = appearance::DecorationColors::from_settings(settings);
        Self {
            title_bar_focused: color_to_argb(&colors.title_focused_bg),
            title_bar_unfocused: color_to_argb(&colors.title_unfocused_bg),
            title_text_focused: color_to_argb(&colors.title_focused_fg),
            title_text_unfocused: color_to_argb(&colors.title_unfocused_fg),
            close_button: color_to_argb(&colors.close_button),
            maximize_button: color_to_argb(&colors.maximize_button),
            minimize_button: color_to_argb(&colors.minimize_button),
            border_focused: color_to_argb(&colors.border_focused),
            border_unfocused: color_to_argb(&colors.border_unfocused),
            shadow_color: color_to_argb(&colors.shadow),
            desktop_background: color_to_argb(&colors.desktop_bg),
        }
    }
}

impl Default for DecorationTheme {
    /// The palette for the default settings.
    ///
    /// Deferring to [`AppearanceSettings::default`] rather than restating a
    /// palette is what makes a compositor that has never loaded a settings file
    /// look identical to one that loaded a file saying nothing unusual — the
    /// two used to differ, and the difference was only ever visible on screen.
    fn default() -> Self {
        Self::from_settings(&AppearanceSettings::default())
    }
}

// ---------------------------------------------------------------------------
// Compositor
// ---------------------------------------------------------------------------

/// How the most recently presented frame was produced.
///
/// In the [`Direct`](Scanout::Direct) case the displayed pixels come straight
/// from a fullscreen client's shared buffer — the compositor never touched the
/// framebuffer for that frame (true zero-copy direct scanout). Otherwise the
/// frame was composited normally into the framebuffer's front buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scanout {
    /// Frame composited into the framebuffer.
    Composited,
    /// Frame scanned out directly from the named window's shared buffer.
    Direct(WindowId),
}

/// The main compositor state machine.
pub struct Compositor {
    /// All managed windows (ordered by creation, z_order field determines draw order).
    windows: Vec<Window>,
    /// Z-order stack (WindowIds from bottom to top).
    z_stack: Vec<WindowId>,
    /// The currently focused window (receives keyboard input).
    focused_window: Option<WindowId>,
    /// The backend we composite through.
    ///
    /// Named for the seam rather than for the surface: nothing in this struct
    /// may assume the pixels live in local memory, because on a GPU backend
    /// they will not.
    backend: RenderBackend,
    /// Display configuration.
    display_manager: DisplayManager,
    /// Damage tracking for the current frame.
    damage: DamageRegion,
    /// Frame timing statistics.
    frame_stats: FrameStats,
    /// Current mouse cursor position.
    cursor_x: i32,
    /// Current mouse cursor position.
    cursor_y: i32,
    /// Current cursor shape.
    cursor_shape: CursorShape,
    /// Active drag operation (if any).
    drag: Option<DragState>,
    /// Where the window being dragged would land if the user let go now.
    ///
    /// Kept rather than recomputed each frame because the *change* is what has
    /// to be damaged: an outline that moved has to be painted over where it
    /// was, and nothing else in the frame knows it was ever there. Its only
    /// writer is [`set_drag_preview`](Compositor::set_drag_preview), which is
    /// what pairs the two rectangles with the damage they need.
    drag_preview: Option<DropIntent>,
    /// The previous left-press on a title bar, for recognising a double-click.
    ///
    /// The window is part of it, not just the time: two quick clicks on two
    /// different title bars are two clicks, and maximising the second window
    /// because the first was clicked recently is a window moving on its own.
    last_title_press: Option<(WindowId, Instant)>,
    /// How close together two title-bar clicks must be to count as one
    /// double-click.
    ///
    /// Starts at the same default as the mouse settings panel
    /// (`inputsettings::MouseConfig`), and is replaced by the user's own choice
    /// when [`reload_input`](Compositor::reload_input) reads `input.yaml` — at
    /// startup and whenever a `ReloadInput` request arrives.
    double_click_interval: Duration,
    /// Rendering engine instance.
    render_engine: RenderEngine,
    /// Decoration theme.
    theme: DecorationTheme,
    /// The user's appearance preferences, as far as the compositor can act on
    /// them: how round window corners are and whether windows cast shadows.
    ///
    /// Held as the whole [`AppearanceSettings`] rather than as the two fields
    /// used today, because the settings are one document with one owner
    /// (`gui/appearance`), and copying two of its fields out into compositor-
    /// local state is how a third independent appearance model gets started —
    /// the exact thing that crate exists to prevent. The colours in
    /// [`DecorationTheme`] are the next thing to come from here.
    appearance: AppearanceSettings,
    /// The user's input preferences, or `None` until `input.yaml` has been
    /// read once.
    ///
    /// `None` is load-bearing and is not the same as
    /// [`InputSettings::default`]. The input source is constructed with the
    /// user's settings already in hand, and
    /// [`Server::run_with`](crate::Server::run_with) pushes this into it
    /// whenever it differs from what was pushed last. If "not read yet" were
    /// spelled as the defaults, the first tick of a session would push the
    /// defaults over the settings the source was built with — a pointer that
    /// reverted to stock speed for as long as nobody edited the file.
    ///
    /// Held whole rather than as the fields the compositor itself consumes, for
    /// the same reason [`Self::appearance`] is: the file has one owner
    /// (`gui/inputsettings`), and the compositor is a *carrier* for most of it.
    /// Only [`Self::double_click_interval`] and [`Self::layout`] are read
    /// here; pointer speed, acceleration, button mapping and key repeat are
    /// applied by whoever integrates the raw device deltas, which is why they
    /// have to travel.
    input: Option<InputSettings>,
    /// The keyboard layout in force: which letter each physical key produces.
    ///
    /// Resolved once, when the settings are adopted, rather than looked up per
    /// keystroke — the id in `input.yaml` is a name, and turning a name into a
    /// table on every key press would put a string comparison on the path
    /// between a switch closing and a letter appearing.
    ///
    /// Not an `Option`: unlike [`Self::input`], "not read yet" and "US QWERTY"
    /// are the same thing here, because this one is *used* by the compositor
    /// rather than carried through it. A compositor with no layout at all
    /// could not translate the keystroke that opens Settings.
    layout: &'static keylayout::Layout,
    /// Outbound event notifications for clients (stub queue).
    pending_notifications: VecDeque<EventNotification>,
    /// Reused encoding buffer for
    /// [`route_window_list`](Self::route_window_list), so that a shell polling
    /// an unchanged desktop at 60 Hz allocates nothing.
    window_list_scratch: Vec<u8>,
    /// Which modifier keys are held, and whether Caps Lock is latched.
    ///
    /// Kept here rather than derived per event because a modifier is a *state*
    /// spanning two events — a press and a release, arbitrarily far apart —
    /// and the chord a client is told about is the state at the moment the
    /// other key went down. Held centrally for the same reason the keymap is
    /// (`design-decisions.md` §456): one answer for the whole system, so two
    /// applications cannot disagree about whether Ctrl was down.
    modifiers: ModifierState,
    /// The dead-key accent waiting for the keystroke that completes it.
    ///
    /// Beside [`Self::modifiers`] and for the same reason: it is keyboard
    /// state spanning two events, and one answer for the whole desktop is the
    /// point. See the [`deadkey`] module docs for the rules.
    dead_keys: deadkey::DeadKeys,
    /// Whether a full recomposite is needed (e.g., after display resize).
    full_recomposite: bool,
    /// Whether [`render_all_windows`](Self::render_all_windows) may skip the
    /// parts of a window that windows above it opaquely cover.
    ///
    /// Always on in production. It exists as a switch so a test can composite
    /// the same scene with and without the cull and compare the framebuffers
    /// pixel for pixel — an optimization that changes the image is a bug, and
    /// the only way to say so is to have the unculled image to compare against.
    occlusion_cull: bool,
    /// How the last presented frame was produced (composited vs direct scanout).
    scanout: Scanout,
    /// Active remote draw-command stream sessions, keyed by stream id. Each
    /// tracks its own per-window delta state so multiple remote viewers can be
    /// served independently.
    stream_sessions: BTreeMap<u64, SceneSession>,
    /// Monotonic allocator for stream session ids.
    next_stream_id: u64,
    /// Which virtual desktop is being shown.
    ///
    /// The compositor deliberately has no idea how many there are: a count is a
    /// user preference, it belongs to whatever is offering the user the choice,
    /// and a second copy of it here would be a second answer to drift from the
    /// shell's. What the compositor owns is which one is *showing*, because
    /// that is the part that decides pixels.
    current_workspace: u32,
}

impl Compositor {
    /// Create a new compositor with the given display dimensions.
    pub fn new(width: u32, height: u32, refresh_rate: u32) -> CompositorResult<Self> {
        let backend = RenderBackend::software(width, height)?;
        let display_manager = DisplayManager::new(width, height, refresh_rate);
        let frame_interval = frame_interval_for(refresh_rate);

        Ok(Self {
            windows: Vec::new(),
            z_stack: Vec::new(),
            focused_window: None,
            backend,
            display_manager,
            damage: DamageRegion::new(),
            frame_stats: FrameStats::new(frame_interval),
            cursor_x: width as i32 / 2,
            cursor_y: height as i32 / 2,
            cursor_shape: CursorShape::Arrow,
            drag: None,
            drag_preview: None,
            last_title_press: None,
            double_click_interval: Duration::from_millis(u64::from(DEFAULT_DOUBLE_CLICK_MS)),
            render_engine: RenderEngine::new(),
            theme: DecorationTheme::default(),
            // The defaults, not the user's file: a constructor that read
            // `$HOME` would make every test of this crate depend on the machine
            // running it. `main` loads the file and calls `set_appearance`.
            appearance: AppearanceSettings::default(),
            // `None`, not the defaults, and for a sharper reason than the line
            // above: this is pushed into the input source, so "the defaults"
            // would be a push that overwrites the settings that source was
            // built with. Not knowing cannot overwrite anything.
            input: None,
            layout: keylayout::default_layout(),
            pending_notifications: VecDeque::new(),
            window_list_scratch: Vec::new(),
            modifiers: ModifierState::new(),
            dead_keys: deadkey::DeadKeys::new(),
            full_recomposite: true,
            occlusion_cull: true,
            scanout: Scanout::Composited,
            stream_sessions: BTreeMap::new(),
            next_stream_id: 1,
            current_workspace: 0,
        })
    }

    // -----------------------------------------------------------------------
    // Appearance
    // -----------------------------------------------------------------------

    /// Adopt the user's appearance preferences.
    ///
    /// Forces a full recomposite, because the settings that reach here change
    /// pixels *outside* any window's damage: turning shadows off leaves the old
    /// shadow lying on the desktop until something else happens to repaint that
    /// strip, and squaring a corner leaves the quarter-disc of frame colour that
    /// used to fill it. Nothing marks those regions dirty — no window moved —
    /// so the repaint has to be asked for here.
    ///
    /// *Unless nothing changed*, in which case there is nothing to repaint and
    /// the damage state is left alone. This is the one place that comparison
    /// belongs: [`reload_appearance`](Self::reload_appearance) can be sent by
    /// any connected client at any rate, and a full-screen repaint per request
    /// would make a harmless notification into a way to keep the compositor
    /// redrawing the whole desktop for nothing.
    pub fn set_appearance(&mut self, settings: AppearanceSettings) {
        if settings == self.appearance {
            return;
        }
        self.appearance = settings;
        // Resolved once here rather than per frame: the packing is arithmetic
        // on eleven colours, and they change only when this is called.
        self.theme = DecorationTheme::from_settings(&self.appearance);
        self.full_recomposite = true;
    }

    /// How close together two title-bar clicks must be to maximize the window.
    ///
    /// Clamped to [`MIN_DOUBLE_CLICK_MS`]..=[`MAX_DOUBLE_CLICK_MS`].
    pub fn set_double_click_ms(&mut self, ms: u32) {
        self.double_click_interval = Duration::from_millis(u64::from(
            ms.clamp(MIN_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS),
        ));
    }

    /// The double-click window currently in force, in milliseconds.
    ///
    /// Reported rather than left to the private field so that a caller — and a
    /// test — can see what a reload actually adopted. The value came through
    /// [`set_double_click_ms`](Self::set_double_click_ms), so it is always
    /// within the supported range.
    #[must_use]
    pub fn double_click_ms(&self) -> u32 {
        // The interval is only ever set from a `u32` count of milliseconds
        // clamped to at most `MAX_DOUBLE_CLICK_MS`, so the conversion back
        // cannot fail; the saturating form avoids a panic path for a case the
        // constructor and the one setter between them make unreachable.
        u32::try_from(self.double_click_interval.as_millis()).unwrap_or(u32::MAX)
    }

    /// Re-read the user's `appearance.yaml` and adopt whatever it now says.
    ///
    /// This is the only place in the library that touches the filesystem, and
    /// the asymmetry with [`Compositor::new`] is deliberate rather than
    /// inconsistent: a *constructor* that consulted `$HOME` would make every
    /// test in this crate depend on the machine running it, whereas re-reading
    /// the user's file is the entire meaning of this call — a caller that did
    /// not want it would not have made it. Tests point it somewhere harmless
    /// with `appearance::config::testing::with_scratch_config`.
    ///
    /// A missing or unreadable file yields the defaults, exactly as at startup:
    /// a user who has never opened the Personalization page is the ordinary
    /// case, not a failure to report.
    pub fn reload_appearance(&mut self) {
        self.set_appearance(appearance::AppearanceFile::load().settings);
    }

    /// Re-read the user's `input.yaml` and adopt whatever it now says.
    ///
    /// The counterpart of [`reload_appearance`](Self::reload_appearance), with
    /// the same asymmetry against the constructor and for the same reason, and
    /// pointed somewhere harmless in tests by the same
    /// `inputsettings::config::testing::with_scratch_config`.
    ///
    /// **What it applies, and what it carries.** The compositor is the consumer
    /// of exactly one of these settings — the double-click window — and it
    /// applies that one directly. The rest it *carries*: pointer speed,
    /// acceleration, button mapping and key repeat are applied where raw device
    /// deltas arrive, because a relative delta is only a pointer position once
    /// something has integrated it, and the thing integrating it is the input
    /// source. So the whole file is kept in [`Self::input_settings`], from
    /// which [`Server::run_with`](crate::Server::run_with) pushes it into the
    /// source via [`Present::reload_input`](present::Present::reload_input).
    ///
    /// That carrying is the whole point of the method. Before it existed this
    /// read the file and threw away everything but the double-click window, so
    /// the Settings panel's pointer-speed slider wrote a value that nothing
    /// read until the next login — a control that appeared not to work.
    ///
    /// Unlike an appearance reload this never repaints: no pixel on the screen
    /// depends on how long a double click may take, and the settings that do
    /// change what the user sees change it by moving the pointer, not by
    /// redrawing it.
    pub fn reload_input(&mut self) {
        self.set_input_settings(inputsettings::InputFile::load().settings);
    }

    /// Adopt the given input preferences, without reading any file.
    ///
    /// The counterpart of [`set_appearance`](Self::set_appearance), split from
    /// [`reload_input`](Self::reload_input) for the same reason that one is
    /// split from `reload_appearance`: where the settings came from is the
    /// caller's business, and a test that had to write `$HOME/input.yaml` to
    /// check that a pointer speed travels would be testing the config crate.
    ///
    /// Unlike `set_appearance` this does not compare before storing and does
    /// not repaint. There is nothing to repaint, and the one reader that could
    /// be made to do redundant work by a repeated store —
    /// [`Server::run_with`](crate::Server::run_with) — does its own comparison,
    /// because it has to: it must also not act on the *first* value if that
    /// value is the one the input source already has.
    pub fn set_input_settings(&mut self, settings: InputSettings) {
        self.set_double_click_ms(settings.mouse.double_click_ms);
        // An id the catalogue does not contain keeps the layout that is
        // already in force rather than reverting to US QWERTY. A user who has
        // chosen Dvorak and then acquires an `input.yaml` naming a layout this
        // build has not got should not be silently moved back to QWERTY
        // mid-session; and at startup the layout in force *is* the default, so
        // the fresh-install case is unaffected.
        if let Some(layout) = keylayout::by_id(&settings.keyboard.layout) {
            self.layout = layout;
        }
        self.input = Some(settings);
    }

    /// The keyboard layout in force.
    ///
    /// US QWERTY until `input.yaml` has been read and names another — see
    /// [`Self::layout`] for why this one has no "not read yet" state.
    #[must_use]
    pub const fn keyboard_layout(&self) -> &'static keylayout::Layout {
        self.layout
    }

    /// The input preferences currently in force, or `None` if `input.yaml` has
    /// not been read yet.
    ///
    /// Polled once per tick by [`Server::run_with`](crate::Server::run_with),
    /// which forwards any *change* to the input source. Deliberately polled
    /// rather than pushed, matching
    /// [`Present::monitors`](present::Present::monitors) in the same spirit: a
    /// push needs a queue, and a queue is a thing that can get out of step with
    /// what it describes.
    ///
    /// See [`Self::input`] for why the `None` is not spelled as the defaults.
    #[must_use]
    pub const fn input_settings(&self) -> Option<&InputSettings> {
        self.input.as_ref()
    }

    /// The appearance preferences currently in force.
    #[must_use]
    pub fn appearance(&self) -> &AppearanceSettings {
        &self.appearance
    }

    /// The corner radius for decorations on a display of the given scale.
    ///
    /// Scaled like every other decoration dimension, and for the same reason: a
    /// frame that grows with the display while its corners keep an 8px curve
    /// reads as a frame with sharper corners, not as one drawn at the same size.
    /// Unlike [`scale_dimension`] there is no non-zero floor — a radius of 0 is
    /// the user having chosen [`WindowCorners::Square`], which must survive
    /// scaling as a square corner rather than becoming a 1px curve.
    fn decoration_radius(&self, scale: f32) -> f32 {
        let radius = self.appearance.corner_radius() * scale;
        if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            0.0
        }
    }

    // -----------------------------------------------------------------------
    // Window management
    // -----------------------------------------------------------------------

    /// Create a new window with the ordinary defaults and return its ID.
    ///
    /// Kept as the short form for the compositor's own windows and for tests;
    /// a client's window arrives through [`create_window_from_spec`] with the
    /// terms it asked for.
    ///
    /// [`create_window_from_spec`]: Self::create_window_from_spec
    pub fn create_window(
        &mut self,
        title: String,
        width: u32,
        height: u32,
        client_pid: u64,
    ) -> WindowId {
        self.create_window_from_spec(&WindowSpec::new(title, width, height), client_pid)
    }

    /// Create a window on the terms a client asked for, and return its ID.
    ///
    /// The requested size is clamped to the window's own declared limits
    /// immediately, so a spec whose `width` is below its `min_size` can never
    /// produce a window that violates the constraint it shipped with.
    pub fn create_window_from_spec(&mut self, spec: &WindowSpec, client_pid: u64) -> WindowId {
        // Honour a requested position; otherwise place the window at a slightly
        // offset position from existing windows so they don't pile up.
        let (x, y) = spec.position.unwrap_or_else(|| {
            // Cascade in 30px steps, restarting every ten windows. The window
            // count is reduced modulo the cycle *before* being scaled, so a
            // client that opens a pathological number of windows shifts the
            // cascade rather than overflowing the multiply.
            const CASCADE_STEP: i32 = 30;
            const CASCADE_CYCLE: usize = 10;
            let slot = i32::try_from(self.windows.len() % CASCADE_CYCLE).unwrap_or(0);
            let offset = slot.saturating_mul(CASCADE_STEP);
            (offset.saturating_add(100), offset.saturating_add(80))
        });

        let mut window = Window::from_spec(spec, x, y, client_pid);
        let (w, h) = window.clamp_size(window.width, window.height);
        window.width = w;
        window.height = h;
        // The desktop the user is looking at, which is where a window that just
        // opened belongs. A client cannot ask for a workspace in its spec on
        // purpose: choosing which desktop a program appears on is the user's,
        // and a client that could pick would be able to open a window somewhere
        // the user is not looking.
        window.workspace = self.current_workspace;
        let id = window.id;

        self.windows.push(window);
        self.raise_within_layer(id);

        // Focus the new window.
        self.focus_window(id);

        // Mark damage for the new window's area.
        self.damage_window(id);
        self.full_recomposite = true;

        id
    }

    /// Destroy a window.
    pub fn destroy_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        let idx = self
            .window_index(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;

        // Mark the old area as damaged before removing.
        self.damage_window(window_id);

        // A drag on a window that no longer exists has nothing left to move,
        // and its drop preview would go on offering to tile a window the user
        // just closed. Cancelled here rather than tolerated downstream because
        // the preview is drawn from this state every frame.
        if self.drag.as_ref().is_some_and(|d| d.window_id == window_id) {
            self.drag = None;
            self.set_drag_preview(None);
        }

        let closed_layer = self.layer_of(window_id);
        self.windows.remove(idx);
        self.z_stack.retain(|&id| id != window_id);
        self.update_z_orders();

        // If this was the focused window, focus the topmost remaining window
        // *at or below the closed window's band*. Taking the topmost window
        // outright would mean closing an application hands focus to the
        // taskbar, which is in front of everything by construction and is
        // never what the user was looking at next.
        if self.focused_window == Some(window_id) {
            self.focused_window = None;
            if let Some(&next) = self
                .z_stack
                .iter()
                .rev()
                .find(|&&id| self.layer_of(id) <= closed_layer)
            {
                self.focus_window(next);
            }
        }

        self.full_recomposite = true;
        Ok(())
    }

    /// Move a window to a new position.
    pub fn move_window(&mut self, window_id: WindowId, x: i32, y: i32) -> CompositorResult<()> {
        // Damage old position.
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.x = x;
        window.y = y;
        window.dirty = true;

        // Damage new position.
        self.damage_window(window_id);
        Ok(())
    }

    /// Resize a window's client area.
    pub fn resize_window(
        &mut self,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> CompositorResult<()> {
        // Damage old area.
        self.damage_window(window_id);

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;
            // The client's own declared limits bound this, not just the global
            // floor: a window that said it is unusable below 400x300 gets 400x300
            // however small the drag went.
            let (w, h) = window.clamp_size(width, height);
            window.width = w;
            window.height = h;
            window.dirty = true;
            (w, h)
        };

        // Damage new area.
        self.damage_window(window_id);

        // Notify client of resize.
        self.pending_notifications
            .push_back(EventNotification::WindowResized {
                window_id,
                width: final_w,
                height: final_h,
            });

        Ok(())
    }

    /// Minimize a window.
    pub fn minimize_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.minimized = true;
        window.visible = false;
        window.dirty = true;

        // Focus next window if this was focused.
        if self.focused_window == Some(window_id) {
            self.focused_window = None;
            self.focus_topmost_visible();
        }

        self.full_recomposite = true;
        Ok(())
    }

    /// Maximize a window to fill the monitor it is on.
    ///
    /// Refused for a window the client declared non-resizable: maximising is a
    /// resize, and a window that said it only works at one size means it.
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist, and
    /// [`CompositorError::NotResizable`] if the client declared it fixed-size.
    pub fn maximize_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        let bounds = self
            .work_area_of_window(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        self.maximize_window_within(window_id, work_rect(bounds))
    }

    /// Maximize a window into a work area chosen by the caller.
    ///
    /// Split out for the edge drop, which must use the monitor **the pointer is
    /// over** rather than the one that holds the larger part of the window. A
    /// move drag carries the window with the pointer, so usually those are the
    /// same monitor — but a window grabbed far along a wide title bar trails
    /// the pointer by that offset and can still be wholly on the monitor it
    /// came from at the moment the pointer reaches a band on the next one.
    /// Re-deriving the area from the window there would fill the monitor the
    /// user dragged it *off*. It is also what makes the preview honest, since
    /// the release resolves the very area the outline was drawn from rather
    /// than a second opinion about it. See [`Compositor::drop_intent`].
    fn maximize_window_within(
        &mut self,
        window_id: WindowId,
        display_bounds: Rect,
    ) -> CompositorResult<()> {
        if !self
            .window_ref(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?
            .resizable
        {
            return Err(CompositorError::NotResizable(window_id));
        }

        self.damage_window(window_id);

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            // Not merely `!window.maximized`: a *snapped* window's geometry is
            // also not its own, so recording it here would make "restore" return
            // to half the screen rather than to where the window was before the
            // user started tiling it.
            if !window.maximized && window.snapped.is_none() {
                // Save current geometry for restore.
                window.restore_rect =
                    Some(Rect::new(window.x, window.y, window.width, window.height));
            }

            window.maximized = true;
            window.snapped = None;
            // Inset by this window's own frame, not by the constants: an
            // undecorated window has no frame to leave room for and should
            // fill the display exactly rather than being pushed 30px down.
            let (x, y, fit_w, fit_h) = window.client_geometry_for_frame(display_bounds);
            window.x = x;
            window.y = y;
            // A `max_size` still binds when maximised — a window that cannot
            // usefully be drawn wider stays at its width and is simply anchored
            // at the top-left of the work area.
            let (w, h) = window.clamp_size(fit_w, fit_h);
            window.width = w;
            window.height = h;
            window.dirty = true;
            (w, h)
        };

        self.damage_window(window_id);
        self.full_recomposite = true;

        // Notify client of resize.
        self.pending_notifications
            .push_back(EventNotification::WindowResized {
                window_id,
                width: final_w,
                height: final_h,
            });

        Ok(())
    }

    /// Snap a window to one half of the display.
    ///
    /// The shell asks for the *edge*; the rectangle is worked out here, from the
    /// compositor's own bounds. That split is the point of the operation
    /// existing at all — a shell that computed the rectangle itself would be
    /// placing windows, which is the compositor's job, and would be doing it
    /// against a display size it learned second-hand.
    ///
    /// Refused for a non-resizable window, for
    /// [`maximize_window`](Self::maximize_window)'s reason: snapping is a
    /// resize, and a window that declared one working size means it.
    ///
    /// Snapping a maximized window replaces the maximized state rather than
    /// stacking on top of it, and keeps the *original* `restore_rect` — so
    /// maximize, then snap, then restore returns to where the window was before
    /// any of it, not to the full-screen rectangle it had in between.
    pub fn snap_window(&mut self, window_id: WindowId, edge: SnapEdge) -> CompositorResult<()> {
        let bounds = work_rect(
            self.work_area_of_window(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?,
        );
        self.snap_window_within(window_id, edge, bounds)
    }

    /// Snap a window to one half of a work area chosen by the caller.
    ///
    /// Split out for [`retile_for_work_area_change`](Self::retile_for_work_area_change),
    /// which must halve the monitor whose reservation changed rather than
    /// whichever one the window is found on part-way through being moved. The
    /// same reasoning as [`maximize_window_within`](Self::maximize_window_within);
    /// see that method.
    fn snap_window_within(
        &mut self,
        window_id: WindowId,
        edge: SnapEdge,
        bounds: Rect,
    ) -> CompositorResult<()> {
        // Halve by splitting at the midpoint rather than by giving each side
        // `width / 2`: on an odd width the latter leaves a one-pixel column
        // belonging to neither half, which is a permanently visible seam down
        // the middle of the screen.
        let mid = bounds.width / 2;
        let half = match edge {
            SnapEdge::Left => Rect::new(bounds.x, bounds.y, mid, bounds.height),
            SnapEdge::Right => Rect::new(
                bounds
                    .x
                    .saturating_add(i32::try_from(mid).unwrap_or(i32::MAX)),
                bounds.y,
                bounds.width.saturating_sub(mid),
                bounds.height,
            ),
        };

        self.place_snapped(window_id, half, SnapTarget::Half(edge))
    }

    /// Tile a window into one cell of a named multi-window layout.
    ///
    /// The shell names *which zone of which layout* and never a rectangle; the
    /// rectangle is worked out here, from the compositor's own display bounds,
    /// for [`snap_window`](Self::snap_window)'s reason. `guiremote::zones` is
    /// where the arithmetic lives so that the picker the user aimed at and the
    /// placement they got are computed from one definition rather than two
    /// copies of it.
    ///
    /// Everything else — the non-resizable refusal, replacing rather than
    /// stacking on a previous maximize or snap, keeping the *original*
    /// `restore_rect` — matches [`snap_window`](Self::snap_window) exactly,
    /// because both go through the same placement step.
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist,
    /// [`CompositorError::NotResizable`] if the client declared it fixed-size,
    /// and [`CompositorError::ZoneNotInLayout`] if the layout yielded no
    /// rectangle for the slot — which a well-formed [`SnapSlot`] cannot cause.
    pub fn snap_window_to_zone(
        &mut self,
        window_id: WindowId,
        slot: SnapSlot,
    ) -> CompositorResult<()> {
        let area = self
            .work_area_of_window(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        self.snap_window_to_zone_within(window_id, slot, area)
    }

    /// Tile a window into a zone of a work area chosen by the caller.
    ///
    /// Split out for [`maximize_window_within`](Self::maximize_window_within)'s
    /// reason: an edge drop resolves the slot against the monitor the pointer
    /// is over, which is the monitor the preview was drawn on.
    fn snap_window_to_zone_within(
        &mut self,
        window_id: WindowId,
        slot: SnapSlot,
        area: WorkArea,
    ) -> CompositorResult<()> {
        let zone = slot
            .rect(area)
            .ok_or(CompositorError::ZoneNotInLayout(window_id))?;

        self.place_snapped(window_id, zone_rect(zone), SnapTarget::Zone(slot))
    }

    /// The pixels a tiled window may occupy: **one monitor's**, not the whole
    /// virtual desktop's.
    ///
    /// Which monitor is decided by `rect` — the display it overlaps most, by
    /// [`DisplayManager::display_for`]'s rule — so a caller passes the frame of
    /// the window being tiled, or a one-pixel rect at the cursor for a gesture
    /// that has not settled on a window's monitor yet.
    ///
    /// **The union of every display is the wrong answer, and used to be the
    /// one given here.** Maximizing a window on the second of two monitors made
    /// it as wide as both and moved it onto the first; snapping it left gave it
    /// the whole first monitor plus a strip of the second. Nothing caught it
    /// because a one-monitor desktop cannot tell the two readings apart, which
    /// is what every test had. `virtual_bounds()` survives below only as the
    /// answer for a compositor with no displays connected at all, where it is
    /// the empty rectangle and there is nothing better to say.
    ///
    /// Derived on every call rather than cached, because it is a function of
    /// the display arrangement — which a monitor being unplugged changes with
    /// no notice to anything here. A cached copy would be one hotplug away
    /// from tiling a screen that no longer exists, and would do it silently.
    ///
    /// These are the monitor's *whole* bounds. What a tiled window may actually
    /// have is [`work_area_for`](Self::work_area_for), which is this minus the
    /// strips any panel on the monitor has reserved.
    fn work_bounds_for(&self, rect: Rect) -> Rect {
        self.display_manager
            .display_for(&rect)
            .map_or_else(|| self.display_manager.virtual_bounds(), Display::bounds)
    }

    /// The usable part of the monitor `rect` is on: its bounds, minus the strips
    /// panels have reserved along its edges.
    ///
    /// **This, not [`work_bounds_for`](Self::work_bounds_for), is what tiling
    /// divides up.** A window snapped to the left half used to fill exactly half
    /// the monitor including the rows the taskbar occupied, so its bottom — a
    /// status line, the last row of a list, a scrollbar's arrow — was covered by
    /// the bar. Subtracting here fixes maximize, both half-snaps, every zone
    /// slot and the edge-drag drop at once, because all of them resolve their
    /// rectangle from a [`WorkArea`] and this is where every one of those comes
    /// from.
    ///
    /// Derived on every call rather than cached, so a taskbar that changes
    /// height or moves to another edge is followed with no invalidation step to
    /// forget. Already-tiled windows are a separate matter — they hold a
    /// rectangle, not a rule — and are re-placed by
    /// [`retile_for_work_area_change`](Self::retile_for_work_area_change).
    fn work_area_for(&self, rect: Rect) -> WorkArea {
        let bounds = self.work_bounds_for(rect);
        self.reserved_on(bounds).apply(work_area_of(bounds))
    }

    /// Everything reserved along the edges of the monitor with these bounds.
    ///
    /// A hidden or minimized panel reserves nothing. A strip is kept clear so
    /// that what sits in it stays visible, and nothing is sitting in it while
    /// the window is not on screen — so counting it would shrink the desktop
    /// with no visible cause, which is precisely the confusing failure the
    /// clamp in [`ReservedEdges::apply`] exists to bound.
    ///
    /// O(windows) per call rather than a maintained total. The alternative is a
    /// per-monitor sum updated on every reservation *and* every window move,
    /// hide, destroy and display hotplug — five places to forget — to save a
    /// walk over a list that is short by construction and is already walked on
    /// every frame.
    fn reserved_on(&self, bounds: Rect) -> ReservedEdges {
        let mut reserved = ReservedEdges::none();
        for window in &self.windows {
            let Some((edge, size)) = window.reserved_edge else {
                continue;
            };
            if size == 0 || !window.is_showing(self.current_workspace) {
                continue;
            }
            if self.work_bounds_for(window.frame_rect()) == bounds {
                reserved.add(edge, size);
            }
        }
        reserved
    }

    /// Reserve a strip along one edge of the monitor `window_id` is on, and
    /// answer with the work area that leaves.
    ///
    /// This is what a taskbar or dock calls so that tiled windows stop short of
    /// it. `size` is a thickness in pixels; zero releases a reservation made
    /// earlier. A second call for the same window **replaces** its previous
    /// claim rather than adding to it — a panel that changes height sends the
    /// new number and nothing else — while claims from *different* windows on
    /// the same edge add up. See `guiremote::reserve` for why that, and not
    /// X11's side-by-side spans.
    ///
    /// The area returned is what was actually granted, which may be less than
    /// was asked for: a claim is clamped to
    /// [`MAX_RESERVED_FRACTION`](guiremote::reserve::MAX_RESERVED_FRACTION) of
    /// the monitor so that no client can shrink everyone's tiling to nothing.
    /// It also accounts for *other* panels on the same monitor, which is the
    /// honest answer to "where may I put myself" and is not derivable from the
    /// caller's own request.
    ///
    /// Windows already tiled on the affected monitor are re-placed, because a
    /// tiled window holds a rectangle rather than a rule — see
    /// [`retile_for_work_area_change`](Self::retile_for_work_area_change).
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist.
    pub fn reserve_edge(
        &mut self,
        window_id: WindowId,
        edge: PanelEdge,
        size: u32,
    ) -> CompositorResult<WorkArea> {
        // Read before the write: the monitor is the one the panel is on *now*,
        // and a release has to re-tile the monitor it is giving pixels back to
        // rather than whichever one a later lookup happens to find.
        let bounds = self.work_bounds_for(
            self.window_ref(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?
                .frame_rect(),
        );

        let before = self.work_area_for(bounds);
        self.window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?
            .reserved_edge = (size > 0).then_some((edge, size));
        let after = self.work_area_for(bounds);

        if after != before {
            self.retile_for_work_area_change(bounds);
        }
        Ok(after)
    }

    /// Re-place every tiled window on the monitor whose work area just changed.
    ///
    /// A window that is maximized or snapped holds a *rectangle*, not the rule
    /// that produced it, so a taskbar appearing or growing leaves every
    /// already-tiled window exactly where it was — underneath the bar, which is
    /// the bug the reservation existed to prevent. Re-running the same request
    /// against the new area is the whole fix, and it works because
    /// [`SnapTarget`] and `maximized` store what was *asked for*.
    ///
    /// Errors from the re-place are dropped deliberately: the only ones
    /// reachable are `WindowNotFound` for a window taken from this very list a
    /// moment ago, and `NotResizable` for a window that could not have been
    /// snapped in the first place. Neither is something the caller — a panel
    /// asking for pixels — can act on, and failing its reservation because some
    /// unrelated window declined to move would be the wrong answer to both.
    fn retile_for_work_area_change(&mut self, bounds: Rect) {
        let affected: Vec<(WindowId, Option<SnapTarget>)> = self
            .windows
            .iter()
            // A fullscreen window is excluded even when `maximized` is also
            // set, which it is for anything maximised before it went
            // fullscreen. Fullscreen is defined against the whole scanout
            // surface and outranks both tiling states while it lasts, so
            // re-tiling one replaces the display-sized geometry that earns it
            // the direct-scanout bypass with a work-area rectangle: a game
            // visibly shrinking away from the screen edges because a taskbar
            // appeared behind it, and never growing back, since nothing
            // re-asserts fullscreen afterwards on this path.
            //
            // `maximized` is deliberately left set rather than cleared, so that
            // leaving fullscreen still finds a tiling state to fall back to.
            .filter(|w| !w.fullscreen)
            .filter(|w| w.maximized || w.snapped.is_some())
            .filter(|w| self.work_bounds_for(w.frame_rect()) == bounds)
            .map(|w| (w.id, w.snapped))
            .collect();

        for (id, target) in affected {
            // Deliberately the `_within` forms against `bounds`. The public
            // forms would re-derive the monitor from the window, and today they
            // would agree — the filter above only kept windows already on
            // `bounds` — so this is not fixing an observable bug. It is
            // refusing to *depend* on that agreement: this loop moves the very
            // windows the re-derivation would measure, so the public forms
            // would make each re-place correct only because of the order the
            // ones before it happened to leave the list in. `bounds` was read
            // once, before any of it, and cannot drift.
            let area = self.work_area_for(bounds);
            let outcome = match target {
                None => self.maximize_window_within(id, work_rect(area)),
                Some(SnapTarget::Zone(slot)) => self.snap_window_to_zone_within(id, slot, area),
                Some(SnapTarget::Half(edge)) => self.snap_window_within(id, edge, work_rect(area)),
            };
            drop(outcome);
        }
    }

    /// The work area of the monitor the pointer is over.
    ///
    /// A one-pixel rectangle rather than a separate lookup, so that "which
    /// monitor is this point on" and "which monitor is this window on" cannot
    /// answer differently about the same pixel.
    fn work_area_at(&self, x: i32, y: i32) -> WorkArea {
        self.work_area_for(Rect::new(x, y, 1, 1))
    }

    /// The work area of the monitor a window is on, or `None` if it is gone.
    fn work_area_of_window(&self, window_id: WindowId) -> Option<WorkArea> {
        Some(self.work_area_for(self.window_ref(window_id)?.frame_rect()))
    }

    /// Move a window into an already-resolved tile rectangle.
    ///
    /// The shared tail of [`snap_window`](Self::snap_window) and
    /// [`snap_window_to_zone`](Self::snap_window_to_zone). The two differ only
    /// in how they arrive at `rect` and what they record in `snapped`; the
    /// bookkeeping below is subtle enough — see the `restore_rect` comment —
    /// that having it in one place rather than two is the point of the split.
    fn place_snapped(
        &mut self,
        window_id: WindowId,
        rect: Rect,
        target: SnapTarget,
    ) -> CompositorResult<()> {
        if !self
            .window_ref(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?
            .resizable
        {
            return Err(CompositorError::NotResizable(window_id));
        }

        self.damage_window(window_id);

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            // Only the first departure from free-floating geometry records where
            // to come back to. Re-snapping an already-snapped window, or
            // snapping a maximized one, must not overwrite it with the geometry
            // it has *because* of the previous snap.
            if !window.maximized && window.snapped.is_none() {
                window.restore_rect =
                    Some(Rect::new(window.x, window.y, window.width, window.height));
            }

            window.maximized = false;
            window.snapped = Some(target);

            let (x, y, fit_w, fit_h) = window.client_geometry_for_frame(rect);
            window.x = x;
            window.y = y;
            let (w, h) = window.clamp_size(fit_w, fit_h);
            window.width = w;
            window.height = h;
            window.dirty = true;
            (w, h)
        };

        self.damage_window(window_id);
        self.full_recomposite = true;

        self.pending_notifications
            .push_back(EventNotification::WindowResized {
                window_id,
                width: final_w,
                height: final_h,
            });

        Ok(())
    }

    /// Restore a window from minimized, maximized or snapped state.
    ///
    /// The saved rectangle is a rectangle on the desktop *as it was when the
    /// window was tiled*, and the desktop may not be that shape any more — a
    /// resolution change is the obvious way, and it does not help that
    /// [`resize_display`](Self::resize_display) rescues stranded windows, since
    /// a maximised window is not stranded and its saved rectangle is not where
    /// it is. Restoring it verbatim would drop the window somewhere the user
    /// cannot see, click or drag it, and there is no second chance: the saved
    /// rectangle is consumed by the restore, so nothing afterwards knows the
    /// window was ever anywhere else.
    pub fn restore_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        // Read before the window is borrowed mutably. `home` is deliberately the
        // display the window is on *now*: it is currently tiled, so it is
        // demonstrably on a real screen, and un-maximising a window on the
        // second monitor must not move it to the first.
        let Some(frame_now) = self.window_ref(window_id).map(Window::frame_rect) else {
            return Err(CompositorError::WindowNotFound(window_id));
        };
        let desktop = self.display_manager.virtual_bounds();
        let home = self.work_bounds_for(frame_now);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;

        if window.minimized {
            window.minimized = false;
            window.visible = true;
        }

        // Maximized and snapped are alternatives, not stages: either one is a
        // departure from the window's own geometry, and either one is undone by
        // putting `restore_rect` back. Testing them together rather than in
        // sequence is what stops a snapped window from being left in place
        // because it did not happen to also be maximized.
        if window.maximized || window.snapped.is_some() {
            window.maximized = false;
            window.snapped = None;
            if let Some(restore) = window.restore_rect.take() {
                // After the flags are cleared, so that the frame insets are the
                // ones the restored window will actually have.
                let placed = kept_reachable(window.frame_rect_for_client(restore), desktop, home);
                let (x, y, _, _) = window.client_geometry_for_frame(placed);
                window.x = x;
                window.y = y;
                window.width = restore.width;
                window.height = restore.height;
            }
        }

        window.dirty = true;
        self.damage_window(window_id);
        self.focus_window(window_id);
        self.full_recomposite = true;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Fullscreen / direct-scanout bypass
    // -----------------------------------------------------------------------

    /// Enter or leave true fullscreen for a window.
    ///
    /// Entering saves the window's geometry, removes decorations, and resizes
    /// the client area to cover **the monitor the window is on** — not the
    /// framebuffer, which on a multi-head desktop is the union of every monitor
    /// and belongs to no one screen. This is the same question
    /// [`maximize_window`](Self::maximize_window) asks, answered by the same
    /// [`work_bounds_for`](Self::work_bounds_for), and the two differ only in
    /// that maximising yields the panels their reserved strips and fullscreen
    /// covers them.
    ///
    /// Leaving restores the saved geometry — subject to it still being
    /// somewhere the user can reach, for the reason given on
    /// [`restore_window`](Self::restore_window): a game left fullscreen across a
    /// resolution change would otherwise be put back at a rectangle on a screen
    /// that no longer exists, and the saved rectangle is consumed on the way
    /// out, so nothing afterwards could recover it. A fullscreen window with an
    /// opaque, display-sized shared buffer is eligible for direct-scanout
    /// bypass (see [`compose_frame`]) — which, correctly, means only a window
    /// fullscreen on a *single-head* desktop, since covering one monitor of
    /// several does not cover the scanout surface.
    ///
    /// [`compose_frame`]: Compositor::compose_frame
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist.
    pub fn set_fullscreen(&mut self, window_id: WindowId, enable: bool) -> CompositorResult<()> {
        self.damage_window(window_id);

        // The monitor the window is on, read before the mutable borrow. It
        // answers both of this function's questions: which screen a window
        // going fullscreen should cover, and — as in `restore_window` — which
        // screen one coming back out falls back to if its saved rectangle is
        // no longer anywhere reachable.
        let screen = self
            .window_ref(window_id)
            .map(|w| self.work_bounds_for(w.frame_rect()));
        let desktop = self.display_manager.virtual_bounds();

        let resized = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            if enable {
                if !window.fullscreen {
                    window.fs_restore_rect =
                        Some(Rect::new(window.x, window.y, window.width, window.height));
                }
                // The monitor, not the framebuffer. With one screen at the
                // origin those are the same rectangle, which is why this went
                // unnoticed; with two, sizing from the framebuffer made a
                // window fullscreened on the second monitor jump to the first
                // and span both, while `maximize_window` — one screen's work
                // area, via the same `work_bounds_for` — stayed put. Two
                // commands a user thinks of as the same gesture disagreeing
                // about which monitor they act on is not a defensible split.
                let bounds = screen.unwrap_or(desktop);
                window.fullscreen = true;
                window.x = bounds.x;
                window.y = bounds.y;
                window.width = bounds.width;
                window.height = bounds.height;
                window.dirty = true;
                Some((bounds.width, bounds.height))
            } else if window.fullscreen {
                window.fullscreen = false;
                let restored = window.fs_restore_rect.take();
                if let Some(r) = restored {
                    // After `fullscreen` is cleared, so the insets are the ones
                    // the restored window will have -- a fullscreen window is
                    // undecorated, and measuring its frame while it still is
                    // would be measuring the wrong box.
                    let placed = kept_reachable(
                        window.frame_rect_for_client(r),
                        desktop,
                        screen.unwrap_or(desktop),
                    );
                    let (x, y, _, _) = window.client_geometry_for_frame(placed);
                    window.x = x;
                    window.y = y;
                    window.width = r.width;
                    window.height = r.height;
                }
                window.dirty = true;
                restored.map(|r| (r.width, r.height))
            } else {
                None
            }
        };

        self.damage_window(window_id);
        self.full_recomposite = true;

        if let Some((w, h)) = resized {
            self.pending_notifications
                .push_back(EventNotification::WindowResized {
                    window_id,
                    width: w,
                    height: h,
                });
        }

        Ok(())
    }

    /// Whether a window is currently in fullscreen mode.
    #[must_use]
    pub fn is_fullscreen(&self, window_id: WindowId) -> bool {
        self.window_ref(window_id).is_some_and(|w| w.fullscreen)
    }

    /// Determine whether the current frame can bypass compositing via direct
    /// scanout, returning the window whose shared buffer should be scanned out.
    ///
    /// All conditions must hold: the topmost visible window is fullscreen and
    /// fully opaque, covers the entire framebuffer, has an attached shared
    /// buffer whose dimensions exactly match the display, and nothing visible
    /// sits above it in the z-order. When eligible, the compositor presents the
    /// client's buffer pixels directly — no per-frame blit, no occluded windows
    /// drawn. A buffer smaller/larger than the display is rejected (a partial
    /// buffer would leave the rest of the screen stale), preserving correctness.
    ///
    /// **The framebuffer, deliberately, and not the window's own monitor.** On a
    /// multi-head desktop the scanout surface is the union of every screen, so a
    /// window fullscreen on one of them does not cover it and is declined here —
    /// which is the right answer, because the bypass hands the client's buffer
    /// to *every* head, and the second monitor would show the game's pixels
    /// instead of the desktop. The bypass is therefore a single-head
    /// optimisation, and the test that makes it one is this one rather than an
    /// explicit head count: with one screen at the origin the two rectangles are
    /// equal and nothing changes.
    fn direct_scanout_window(&self) -> Option<WindowId> {
        // Topmost visible window in z-order (z_stack top == last).
        let &top = self.z_stack.iter().rev().find(|&&id| {
            self.window_ref(id)
                .is_some_and(|w| w.is_showing(self.current_workspace))
        })?;

        let win = self.window_ref(top)?;
        if !win.fullscreen || win.opacity < 1.0 {
            return None;
        }
        // Must cover the whole framebuffer.
        if win.x > 0 || win.y > 0 {
            return None;
        }
        let covers_w =
            win.x.saturating_add(win.width as i32) as i64 >= self.backend.size().0 as i64;
        let covers_h =
            win.y.saturating_add(win.height as i32) as i64 >= self.backend.size().1 as i64;
        if !covers_w || !covers_h {
            return None;
        }
        // The attached buffer must match the display exactly for a valid,
        // fully-covering scanout.
        let buf = win.buffer.as_ref()?;
        if (buf.width(), buf.height()) == self.backend.size() {
            Some(top)
        } else {
            None
        }
    }

    /// Set focus to a specific window.
    pub fn focus_window(&mut self, window_id: WindowId) {
        let old_focused = self.focused_window;

        // Unfocus the previously focused window.
        if let Some(old_id) = old_focused
            && old_id != window_id
        {
            // A dead-key accent belongs to the window it was armed in. Left
            // pending across a focus change it would complete itself in the
            // *next* window, putting a letter the user never typed into a
            // document they had already left.
            self.dead_keys.cancel();
            if let Some(win) = self.window_mut(old_id) {
                win.focused = false;
                win.dirty = true;
            }
            self.damage_window(old_id);
            self.pending_notifications
                .push_back(EventNotification::FocusLost { window_id: old_id });
        }

        // Focus the new window — unless it is not on screen. A minimized
        // window and a window on another workspace are refused for the same
        // reason: the keyboard would be going somewhere the user cannot see,
        // and no amount of typing would reveal where. `activate_window` is the
        // verb that means "make it reachable *and* focus it", and it undoes
        // both kinds of hiding before calling this.
        let workspace = self.current_workspace;
        if let Some(win) = self.window_mut(window_id)
            && win.is_showing(workspace)
        {
            win.focused = true;
            win.dirty = true;
            self.focused_window = Some(window_id);

            // Bring to the top of its own band — not to the top of the whole
            // stack, which would let any application window climb over the
            // taskbar simply by being clicked.
            self.raise_within_layer(window_id);

            self.damage_window(window_id);
            self.pending_notifications
                .push_back(EventNotification::FocusGained { window_id });
        }
    }

    /// Bring a window to the user: un-minimize it and switch to its workspace
    /// if either is hiding it, then focus and raise it within its band.
    ///
    /// What a taskbar button and an Alt-Tab switcher do, and the reason it is
    /// one operation rather than two: [`focus_window`](Self::focus_window)
    /// deliberately refuses a window that is not on screen — a window nobody
    /// can see must not hold the keyboard — so the un-hiding has to come
    /// *first*. A caller issuing the two separately would be depending on that
    /// order without anything stating it, and would silently do nothing to a
    /// hidden window if it got them the wrong way round.
    ///
    /// **It follows the window to another virtual desktop rather than dragging
    /// the window to this one.** Activating is a request to *see* a particular
    /// window, and the two ways to grant it differ in what happens to
    /// everything else: switching moves one thing (which desktop is showing)
    /// and is undone by switching back, whereas moving the window rearranges
    /// the desktops themselves and leaves the user to notice and repair it.
    /// This is also what every desktop that has workspaces does.
    ///
    /// Distinct from [`restore_window`](Self::restore_window), which also
    /// un-*maximizes*. A taskbar button on a minimized-while-maximized window
    /// must give the window back exactly as the user left it; restoring it
    /// would un-maximize a window the user never asked to un-maximize.
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist —
    /// which for a shell is an ordinary race, not a fault: a window may close
    /// between the list snapshot the button was drawn from and the click.
    pub fn activate_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        let (layer, workspace) = (window.layer, window.workspace);
        if window.minimized {
            window.minimized = false;
            window.visible = true;
            window.dirty = true;
            self.full_recomposite = true;
        }
        if matches!(layer, Layer::Normal) {
            self.switch_workspace(workspace);
        }
        self.damage_window(window_id);
        self.focus_window(window_id);
        Ok(())
    }

    /// Which virtual desktop is being shown.
    #[must_use]
    pub const fn current_workspace(&self) -> u32 {
        self.current_workspace
    }

    /// Show a different virtual desktop.
    ///
    /// Every [`Layer::Normal`] window assigned elsewhere stops being drawn,
    /// stops taking clicks and stops being an occluder in the same instant,
    /// because all three read [`Window::is_showing`]. Nothing else about a
    /// window changes: it is not minimized, not unmapped, not moved, and its
    /// client is not told, because from the client's point of view nothing has
    /// happened that it could act on — the user looked away.
    ///
    /// Focus follows the screen. A window on the desktop being left cannot keep
    /// the keyboard (that is a window nobody can see swallowing every
    /// keystroke), so focus moves to the topmost window that *is* showing, and
    /// to nothing at all on an empty desktop.
    ///
    /// A no-op when the named workspace is already the one showing, so a shell
    /// re-asserting its state costs nothing. There is no upper bound to check
    /// against: see [`Compositor::current_workspace`]'s field.
    pub fn switch_workspace(&mut self, workspace: u32) {
        if workspace == self.current_workspace {
            return;
        }
        self.current_workspace = workspace;
        // Every window on both desktops changes, and none of them is dirty in
        // the damage sense — no window moved. Only a full recomposite repaints
        // the desktop the departing windows were standing on.
        self.full_recomposite = true;
        if let Some(focused) = self.focused_window
            && !self
                .window_ref(focused)
                .is_some_and(|w| w.is_showing(workspace))
        {
            if let Some(win) = self.window_mut(focused) {
                win.focused = false;
                win.dirty = true;
            }
            self.focused_window = None;
            self.pending_notifications
                .push_back(EventNotification::FocusLost { window_id: focused });
            self.focus_topmost_visible();
        }
    }

    /// Put a window on a virtual desktop.
    ///
    /// Takes effect immediately: moving the focused window off the desktop
    /// being shown hides it, and the keyboard goes to whatever is left, by the
    /// same rule [`switch_workspace`](Self::switch_workspace) follows.
    ///
    /// Accepted but meaningless for a window outside [`Layer::Normal`] — the
    /// assignment is stored and [`Window::is_showing`] ignores it, so a taskbar
    /// stays on every desktop no matter what it is told. Refusing it would make
    /// a shell that moves "all its windows" have to know which of them are
    /// furniture.
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist, which
    /// for a shell acting on a snapshot is an ordinary race.
    pub fn set_window_workspace(
        &mut self,
        window_id: WindowId,
        workspace: u32,
    ) -> CompositorResult<()> {
        let showing = self.current_workspace;
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        if window.workspace == workspace {
            return Ok(());
        }
        window.workspace = workspace;
        window.dirty = true;
        let vanished = !window.is_showing(showing);
        self.full_recomposite = true;
        self.damage_window(window_id);
        if vanished && self.focused_window == Some(window_id) {
            if let Some(win) = self.window_mut(window_id) {
                win.focused = false;
            }
            self.focused_window = None;
            self.pending_notifications
                .push_back(EventNotification::FocusLost { window_id });
            self.focus_topmost_visible();
        }
        Ok(())
    }

    /// Ask a window to close, as its own close button does.
    ///
    /// Sends the client [`EventNotification::WindowClose`] rather than
    /// destroying the window: the client is being *told*, so an editor with
    /// unsaved changes gets to put up its dialog. A shell that could destroy a
    /// window outright would be able to discard a user's work from a context
    /// menu.
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist. The
    /// check is what stops a notification being queued for an id nothing will
    /// ever deliver.
    pub fn request_close(&mut self, window_id: WindowId) -> CompositorResult<()> {
        if self.window_ref(window_id).is_none() {
            return Err(CompositorError::WindowNotFound(window_id));
        }
        self.pending_notifications
            .push_back(EventNotification::WindowClose { window_id });
        Ok(())
    }

    /// Set a window's title.
    pub fn set_title(&mut self, window_id: WindowId, title: String) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.title = title;
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    /// Show or hide a window without destroying it.
    ///
    /// Distinct from minimizing: a hidden window is not on the taskbar and is
    /// not something the user can bring back — it is off screen because the
    /// application said so. Showing one therefore clears `minimized` as well,
    /// since a window the client has explicitly asked to be visible should not
    /// stay collapsed for a reason the client cannot see.
    pub fn set_visible(&mut self, window_id: WindowId, visible: bool) -> CompositorResult<()> {
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.visible = visible;
        if visible {
            window.minimized = false;
        }
        window.dirty = true;

        // Hiding the focused window leaves focus nowhere; hand it to whatever
        // is now on top rather than leaving keystrokes going to a window the
        // user cannot see.
        if !visible && self.focused_window == Some(window_id) {
            self.focused_window = None;
            self.focus_topmost_visible();
        }

        self.damage_window(window_id);
        // A window that disappeared exposes whatever was beneath it, which the
        // per-window damage pass will not redraw because it skips invisible
        // windows.
        self.full_recomposite = true;
        Ok(())
    }

    /// Set the cursor a window asks for over its own client area.
    ///
    /// Takes effect on screen immediately only if the pointer is already over
    /// that window; otherwise it is remembered and applies when the pointer
    /// arrives. A client cannot move the cursor it is not under — which is the
    /// point, since the previous global assignment let any client repaint the
    /// desktop cursor from anywhere on the desktop.
    pub fn set_cursor(&mut self, window_id: WindowId, cursor: CursorShape) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.cursor = cursor;

        let (x, y) = (self.cursor_x, self.cursor_y);
        self.update_cursor_shape(x, y);
        Ok(())
    }

    /// Set a window's opacity.
    pub fn set_opacity(&mut self, window_id: WindowId, opacity: f32) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.opacity = opacity.clamp(0.0, 1.0);
        window.dirty = true;
        self.damage_window(window_id);
        self.full_recomposite = true;
        Ok(())
    }

    /// Submit render commands from a client for its window.
    pub fn submit_render(
        &mut self,
        window_id: WindowId,
        commands: Vec<RenderCommand>,
    ) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.render_tree = RenderTree { commands };
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Shared-buffer (DMA-BUF) surface path
    // -----------------------------------------------------------------------

    /// Import a client-shared pixel buffer and attach it to a window.
    ///
    /// While a buffer is attached, the compositor blits it directly into the
    /// window's client area each frame instead of replaying the render tree.
    /// The buffer is validated against hostile geometry by
    /// [`SharedBuffer::import`].
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window is gone, or any error
    /// from [`SharedBuffer::import`] if the client's geometry is invalid.
    pub fn attach_buffer(
        &mut self,
        window_id: WindowId,
        handle: u64,
        width: u32,
        height: u32,
        stride: u32,
        format: BufferFormat,
        bytes: &[u8],
    ) -> CompositorResult<()> {
        // Validate before touching window state so a bad buffer is a no-op.
        let buffer = SharedBuffer::import(handle, width, height, stride, format, bytes)?;
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.buffer = Some(buffer);
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    /// Detach any shared buffer from a window, reverting to the render-tree
    /// path. Returns the detached buffer's handle if one was attached.
    pub fn detach_buffer(&mut self, window_id: WindowId) -> Option<u64> {
        let handle = {
            let window = self.window_mut(window_id)?;
            let h = window.buffer.take().map(|b| b.handle());
            if h.is_some() {
                window.dirty = true;
            }
            h
        };
        if handle.is_some() {
            self.damage_window(window_id);
        }
        handle
    }

    /// Drain the handles of all buffers the compositor has finished reading
    /// since the last call, clearing their release flags. The IPC layer sends a
    /// `wl_buffer.release`-style notification per handle so clients may reuse
    /// the shared memory.
    pub fn take_released_buffer_handles(&mut self) -> Vec<u64> {
        let mut handles = Vec::new();
        for window in &mut self.windows {
            if let Some(buf) = window.buffer.as_mut()
                && let Some(h) = buf.take_release()
            {
                handles.push(h);
            }
        }
        handles
    }

    // -----------------------------------------------------------------------
    // Input routing
    // -----------------------------------------------------------------------

    /// Process an input event and route it to the appropriate window.
    pub fn handle_input(&mut self, event: InputEvent) {
        // Hit testing derives from `frame_insets`, which is scaled, so input
        // needs the same refresh compositing does — and needs it more often.
        // A drag delivers pointer motion far faster than frames are composed,
        // so a window dragged onto a higher-DPI display would otherwise be hit
        // tested against the *previous* display's title-bar height for the
        // remainder of the frame: the grab would slip out from under the
        // pointer mid-drag, which is the one moment a user would notice.
        self.refresh_window_scales();

        match event {
            InputEvent::MouseMove { x, y } => self.handle_mouse_move(x, y),
            InputEvent::MouseButton {
                button,
                pressed,
                x,
                y,
            } => self.handle_mouse_button(button, pressed, x, y),
            InputEvent::MouseScroll { dx, dy, x, y } => self.handle_mouse_scroll(dx, dy, x, y),
            InputEvent::KeyDown {
                scancode,
                character,
            } => self.handle_key(scancode, true, character),
            InputEvent::KeyUp { scancode } => self.handle_key(scancode, false, None),
            InputEvent::TextInput { text } => self.handle_text_input(&text),
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Handle active drag.
        if let Some(drag) = self.drag.clone() {
            // Saturating: the pointer position arrives from a device or from a
            // client injecting one, and the drag origin was recorded from an
            // equally unbounded position, so the difference is not the
            // compositor's to assume fits.
            let dx = x.saturating_sub(drag.start_mouse.x);
            let dy = y.saturating_sub(drag.start_mouse.y);
            let (start_x, start_y) = (drag.start_window_pos.x, drag.start_window_pos.y);

            if drag.mode == DragMode::MoveWindow {
                let _ = self.move_window(
                    drag.window_id,
                    start_x.saturating_add(dx),
                    start_y.saturating_add(dy),
                );
                // Asked for every motion event and answered locally: the whole
                // reason this decision is the compositor's rather than the
                // shell's is that a round trip here would put network latency
                // inside the one part of the gesture the user watches.
                let preview = self.drop_intent(&drag, x, y);
                self.set_drag_preview(preview);
                return;
            }

            let (h_edge, v_edge) = drag.mode.resize_edges();
            let (start_w, start_h) = drag.start_window_size;
            // The window is asked what size it will accept *before* the origin
            // is derived, so a near-edge drag against a client's `min_size`
            // moves the origin by the size change that actually happened rather
            // than the one requested — otherwise the window creeps sideways
            // while its size stays pinned at the minimum.
            let Some(win) = self.window_ref(drag.window_id) else {
                return;
            };
            let (new_w, new_h) =
                win.clamp_size(h_edge.extent(start_w, dx), v_edge.extent(start_h, dy));
            let new_x = h_edge.origin(start_x, start_w, new_w);
            let new_y = v_edge.origin(start_y, start_h, new_h);

            if (new_x, new_y) != (start_x, start_y) {
                let _ = self.move_window(drag.window_id, new_x, new_y);
            }
            let _ = self.resize_window(drag.window_id, new_w, new_h);
            return;
        }

        // Update cursor shape based on what's under the cursor.
        self.update_cursor_shape(x, y);

        // Route mouse move to the window under the cursor.
        if let Some(window_id) = self.window_at(x, y)
            && let Some(win) = self.window_ref(window_id)
        {
            let (local_x, local_y) = win.local_point(x, y);
            self.pending_notifications
                .push_back(EventNotification::MouseEvent {
                    window_id,
                    x: local_x,
                    y: local_y,
                    kind: MouseEventKind::Move,
                });
        }
    }

    /// What a drag means at the moment the pointer reaches `(x, y)`, or `None`
    /// if letting go there would just leave the window where it was dropped.
    ///
    /// The drag-time preview and the release both ask this, so the outline the
    /// user is shown cannot promise a placement the drop does not make.
    ///
    /// Two things stop it from firing on gestures that are not edge drags:
    ///
    /// - **Only a move.** A resize drag reaching an edge is a user sizing a
    ///   window against that edge by hand. Snapping it would overrule a size
    ///   they had just finished choosing, and would do it at the exact moment
    ///   they let go, when it is too late to aim differently.
    /// - **Only after the pointer has moved.** A press and a release in the
    ///   same pixel is a click. Without this, clicking the title bar of a
    ///   window already sitting against the left edge — anywhere in that
    ///   title bar's leftmost few columns — would tile it, from a gesture the
    ///   user would describe as "I clicked on it".
    #[allow(
        clippy::cast_precision_loss,
        reason = "display coordinates are far inside f32's exact-integer range"
    )]
    fn drop_intent(&self, drag: &DragState, x: i32, y: i32) -> Option<DropIntent> {
        if drag.mode != DragMode::MoveWindow {
            return None;
        }
        if (x, y) == (drag.start_mouse.x, drag.start_mouse.y) {
            return None;
        }
        // The monitor under the *pointer*, not under the window. A move drag
        // carries the window along with the pointer, so for an ordinary grab
        // near the left end of a title bar the two agree — but not always:
        //   - at the interior seam the window straddles both monitors, and
        //     which one holds the larger part is not the one being aimed at;
        //   - a window grabbed far along a wide title bar trails the pointer
        //     by that offset, so it can still be wholly on the monitor it
        //     came from when the pointer has reached a band on the next one.
        // In both cases aiming at a band on screen two must mean screen two,
        // because screen two is the one the preview outline was drawn on.
        // Proved by `a_drop_tiles_the_monitor_the_pointer_is_over_even_when_
        // the_window_is_not` and `the_interior_seam_is_two_edges_and_not_a_
        // middle`.
        let area = self.work_area_at(x, y);
        let drop = guiremote::zones::drop_at(x as f32, y as f32, area)?;
        Some(DropIntent { drop, area })
    }

    /// The rectangle a drop would place the window's *frame* in.
    ///
    /// The same rectangle the drop itself lands in, because the release
    /// resolves the slot against the very [`WorkArea`] carried here rather than
    /// deriving a second one — see [`DropIntent`]. Asserted by
    /// `an_edge_drop_lands_in_the_rectangle_the_drop_promised`.
    fn preview_rect(&self, intent: DropIntent) -> Option<Rect> {
        intent.drop.rect(intent.area).map(zone_rect)
    }

    /// Show, move or take down the drop preview.
    ///
    /// Both the outgoing and the incoming rectangle are damaged, because the
    /// outline is the only thing in the frame that knows where it used to be:
    /// nothing else would repaint the pixels it is leaving, and a preview that
    /// moved would smear a trail of half-drawn outlines across the desktop.
    ///
    /// Guarded on inequality so that the common case — a drag whose pointer is
    /// nowhere near an edge, moving from one nothing to the same nothing — adds
    /// no damage at all.
    fn set_drag_preview(&mut self, next: Option<DropIntent>) {
        if self.drag_preview == next {
            return;
        }
        for intent in [self.drag_preview, next].into_iter().flatten() {
            if let Some(rect) = self.preview_rect(intent) {
                self.damage.add(rect);
            }
        }
        self.drag_preview = next;
    }

    /// Draw the outline of where the dragged window would land.
    ///
    /// A translucent wash under a solid border, in the same colour as a focused
    /// window's frame: the preview is saying "the window you are holding goes
    /// here", so it is drawn in the colour that already means "this one is
    /// yours". Taking the colour from the theme rather than a constant is what
    /// makes it follow the user's accent instead of being a second opinion
    /// about what the desktop looks like.
    fn render_drag_preview(&mut self) {
        let Some(intent) = self.drag_preview else {
            return;
        };
        let Some(rect) = self.preview_rect(intent) else {
            return;
        };
        let color = self.theme.border_focused;
        self.backend.fill_rect(rect, color, PREVIEW_FILL_OPACITY);
        for band in rect_outline(rect, PREVIEW_BORDER_PX) {
            self.backend.fill_rect(band, color, 1.0);
        }
    }

    /// Apply whatever letting go at `(x, y)` means to a drag that is ending.
    ///
    /// Routed through [`maximize_window`](Self::maximize_window) and
    /// [`snap_window_to_zone`](Self::snap_window_to_zone) rather than placing
    /// the rectangle directly, so an edge drop inherits everything a tiling
    /// operation already has to get right: refusing a window the client
    /// declared fixed-size, recording where to restore to without letting a
    /// second snap overwrite the first one's answer, and telling the client it
    /// was resized. Placing the rectangle here would be a third copy of that
    /// bookkeeping, and the one nobody would think to update.
    fn finish_drag(&mut self, drag: &DragState, x: i32, y: i32) {
        // Down first, and unconditionally: the drag is over either way, and a
        // preview left standing would outlive the gesture that explains it.
        self.set_drag_preview(None);
        // Errors are dropped because none of them is actionable at a mouse
        // release: a non-resizable window declining to be tiled is the correct
        // outcome of the gesture, and a window that vanished mid-drag has
        // nothing left to place.
        // The `_within` forms, so that the release places the window in the
        // work area the preview was drawn from. The public forms would look the
        // area up again from the window, which is still on the monitor it was
        // dragged away from.
        match self.drop_intent(drag, x, y) {
            Some(DropIntent {
                drop: EdgeDrop::Maximize,
                area,
            }) => {
                let _ = self.maximize_window_within(drag.window_id, work_rect(area));
            }
            Some(DropIntent {
                drop: EdgeDrop::Zone(slot),
                area,
            }) => {
                let _ = self.snap_window_to_zone_within(drag.window_id, slot, area);
            }
            None => {}
        }
    }

    fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Release ends any active drag.
        if !pressed && button == MouseButton::Left {
            if let Some(drag) = self.drag.take() {
                self.finish_drag(&drag, x, y);
            }
            return;
        }

        if !pressed {
            // Route release to focused window.
            if let Some(window_id) = self.focused_window
                && let Some(win) = self.window_ref(window_id)
            {
                let (local_x, local_y) = win.local_point(x, y);
                self.pending_notifications
                    .push_back(EventNotification::MouseEvent {
                        window_id,
                        x: local_x,
                        y: local_y,
                        kind: MouseEventKind::ButtonRelease(button),
                    });
            }
            return;
        }

        // Left button press: check window decorations first, then client area.
        if button == MouseButton::Left {
            // Taken and cleared before anything is dispatched, so that only a
            // title-bar press can leave one behind. Two title clicks with a
            // press on the desktop between them are two clicks, however fast:
            // the user went somewhere else and came back.
            let previous_title_press = self.last_title_press.take();

            // Check windows from top to bottom z-order.
            let hit_window = self.window_at_with_decorations(x, y);

            if let Some(window_id) = hit_window {
                // Focus the window.
                self.focus_window(window_id);

                // Check if we hit a decoration element.
                if let Some(win) = self.window_ref(window_id) {
                    // Close button?
                    if win.close_button_rect().is_some_and(|r| r.contains(x, y)) {
                        self.pending_notifications
                            .push_back(EventNotification::WindowClose { window_id });
                        return;
                    }
                    // Maximize button?
                    if win.maximize_button_rect().is_some_and(|r| r.contains(x, y)) {
                        if win.maximized {
                            let _ = self.restore_window(window_id);
                        } else {
                            let _ = self.maximize_window(window_id);
                        }
                        return;
                    }
                    // Minimize button?
                    if win.minimize_button_rect().is_some_and(|r| r.contains(x, y)) {
                        let _ = self.minimize_window(window_id);
                        return;
                    }
                    // Title bar: a double-click toggles maximize, a single one
                    // begins a move.
                    if win.title_bar_rect().is_some_and(|r| r.contains(x, y)) {
                        let now = Instant::now();
                        let doubled = previous_title_press.is_some_and(|(prev, at)| {
                            prev == window_id
                                && now.duration_since(at) <= self.double_click_interval
                        });
                        if doubled {
                            // Left cleared by the `take` above rather than
                            // replaced: otherwise a third click pairs with the
                            // second and un-maximizes what the user just
                            // maximized.
                            let maximized = win.maximized;
                            if maximized {
                                let _ = self.restore_window(window_id);
                            } else {
                                let _ = self.maximize_window(window_id);
                            }
                            return;
                        }
                        // Read off the window before recording the press: the
                        // record is a write to `self`, and `win` borrows it.
                        let start_window_pos = Point::new(win.x, win.y);
                        let start_window_size = (win.width, win.height);
                        self.last_title_press = Some((window_id, now));
                        self.drag = Some(DragState {
                            window_id,
                            mode: DragMode::MoveWindow,
                            start_mouse: Point::new(x, y),
                            start_window_pos,
                            start_window_size,
                        });
                        return;
                    }
                    // Border resize?
                    if let Some(mode) = self.detect_border_drag(win, x, y) {
                        self.drag = Some(DragState {
                            window_id,
                            mode,
                            start_mouse: Point::new(x, y),
                            start_window_pos: Point::new(win.x, win.y),
                            start_window_size: (win.width, win.height),
                        });
                        return;
                    }
                    // Client area click.
                    let (local_x, local_y) = win.local_point(x, y);
                    self.pending_notifications
                        .push_back(EventNotification::MouseEvent {
                            window_id,
                            x: local_x,
                            y: local_y,
                            kind: MouseEventKind::ButtonPress(button),
                        });
                }
            }
        } else {
            // Non-left button: route to window under cursor.
            if let Some(window_id) = self.window_at(x, y)
                && let Some(win) = self.window_ref(window_id)
            {
                let (local_x, local_y) = win.local_point(x, y);
                self.pending_notifications
                    .push_back(EventNotification::MouseEvent {
                        window_id,
                        x: local_x,
                        y: local_y,
                        kind: MouseEventKind::ButtonPress(button),
                    });
            }
        }
    }

    fn handle_mouse_scroll(&mut self, dx: f32, dy: f32, x: i32, y: i32) {
        if let Some(window_id) = self.window_at(x, y)
            && let Some(win) = self.window_ref(window_id)
        {
            let (local_x, local_y) = win.local_point(x, y);
            self.pending_notifications
                .push_back(EventNotification::MouseEvent {
                    window_id,
                    x: local_x,
                    y: local_y,
                    kind: MouseEventKind::Scroll { dx, dy },
                });
        }
    }

    fn handle_key(&mut self, scancode: u32, pressed: bool, character: Option<char>) {
        // Folded *before* the notification is built, so a client told about
        // Shift+A sees `shift: true`. Folding afterwards would report the state
        // as it was before the chord completed, which for the modifier key's
        // own event means Shift arriving as unmodified — and any client
        // tracking modifiers from these events would then be permanently one
        // event behind.
        self.modifiers.update(scancode, pressed);

        // Updated unconditionally, even with no focused window: a modifier
        // pressed while focus was elsewhere is still physically down, and
        // skipping it here would leave the state wrong for every later event.
        let Some(window_id) = self.focused_window else {
            return;
        };

        let level = self.modifiers.level();
        let (key, laid_out) = keymap::key_for_layout(self.layout, scancode, level);
        let mut modifiers = self.modifiers.modifiers();
        if keymap::resolves_through_alt_gr(self.layout, scancode, level) {
            // AltGr spent itself selecting a character, so it is not also an
            // Alt chord. Without this a German user typing `@` (AltGr+Q) sends
            // every application an Alt+Q, and the menu bar answers first.
            // Cleared only when the layout really put a character on that
            // level: on a US board AltGr is just the right-hand Alt, and an
            // Alt+Q shortcut must keep working from either side.
            modifiers.alt = self.modifiers.left_alt();
        }
        // The source's own character wins where it has one, and the dead-key
        // machine is skipped with it: a source that hands over a finished
        // character has already run whatever composition its own layout
        // implies, and running ours on top would compose twice. Releases
        // carry no text at all — a key going up inserts nothing, and a
        // character there would have every text field type each letter twice.
        //
        // Everything else goes through `DeadKeys`, which is where a `String`
        // rather than a `char` is finally earned: `´` then `x` types *two*
        // characters (`design-decisions.md` §550), and until this call every
        // keystroke could only ever produce one.
        let text = match (character, pressed) {
            (Some(from_source), true) => from_source.to_string(),
            (None, true) => self.dead_keys.press(
                self.layout,
                scancode,
                level,
                laid_out,
                // After the AltGr fold above, so a German user's AltGr+Q is
                // text entry and not an Alt chord. Super is included because
                // Super+E opens a file manager on every desktop there is; a
                // pending accent must survive that too.
                modifiers.ctrl || modifiers.alt || modifiers.super_key,
            ),
            (_, false) => String::new(),
        };
        self.pending_notifications
            .push_back(EventNotification::KeyEvent {
                window_id,
                scancode,
                key,
                pressed,
                modifiers,
                text,
            });
    }

    /// Release every held modifier.
    ///
    /// Call when the compositor loses the keyboard — session switch, device
    /// unplug, VT change. Without it a modifier held at that moment stays held
    /// forever and every later keystroke arrives as a chord: the classic
    /// "stuck Ctrl" that makes a desktop look crashed when nothing has
    /// actually failed.
    ///
    /// A pending dead-key accent is discarded with them, and for the same
    /// reason: the keyboard is gone, so the vowel that would have completed it
    /// is never coming. Leaving it armed would attach an accent the user typed
    /// in one session to the first letter they type in the next.
    pub const fn release_all_modifiers(&mut self) {
        self.modifiers.release_all();
        self.dead_keys.cancel();
    }

    /// The modifier keys currently held.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers.modifiers()
    }

    fn handle_text_input(&mut self, _text: &str) {
        // Text input is delivered as key events with characters for now.
        // A full IME system would handle this separately.
    }

    // -----------------------------------------------------------------------
    // Hit testing
    // -----------------------------------------------------------------------

    /// Find the topmost window whose client area contains the point.
    fn window_at(&self, x: i32, y: i32) -> Option<WindowId> {
        // Iterate z_stack from top to bottom.
        for &window_id in self.z_stack.iter().rev() {
            if let Some(win) = self.window_ref(window_id)
                && win.is_showing(self.current_workspace)
                && win.client_rect().contains(x, y)
            {
                return Some(window_id);
            }
        }
        None
    }

    /// Find the topmost window whose full area (including decorations) contains the point.
    fn window_at_with_decorations(&self, x: i32, y: i32) -> Option<WindowId> {
        for &window_id in self.z_stack.iter().rev() {
            if let Some(win) = self.window_ref(window_id)
                && win.is_showing(self.current_workspace)
                && win.outer_rect().contains(x, y)
            {
                return Some(window_id);
            }
        }
        None
    }

    /// Detect which border edge the cursor is on (for resize drag detection).
    ///
    /// Returns `None` for a window the client declared non-resizable, and for
    /// an unframed one — there is no border to grab, and the grab band would
    /// otherwise sit inside the client area and steal its clicks.
    fn detect_border_drag(&self, win: &Window, x: i32, y: i32) -> Option<DragMode> {
        if !win.resizable || !win.is_framed() {
            return None;
        }
        let side = win.frame_insets().1;
        let grab = i32::try_from(side.saturating_add(win.shadow_extent())).unwrap_or(i32::MAX);
        let outer = win.outer_rect();

        // Don't detect border drag if the point is inside the client area or title bar.
        if win.client_rect().contains(x, y)
            || win.title_bar_rect().is_some_and(|r| r.contains(x, y))
        {
            return None;
        }

        if !outer.contains(x, y) {
            return None;
        }

        // The grab bands are the outermost `grab` pixels of the frame box — the
        // same box `render_border` strokes, measured the same way. Written out
        // longhand this was a fifth derivation of the frame box, and one that
        // overflowed for a client-chosen origin near the coordinate edge.
        let frame = win.frame_rect();
        let at_left = x < frame.x.saturating_add(grab);
        let at_right = x >= frame.right().saturating_sub(grab);
        let at_top = y < frame.y.saturating_add(grab);
        let at_bottom = y >= frame.bottom().saturating_sub(grab);

        match (at_left, at_right, at_top, at_bottom) {
            (true, false, true, false) => Some(DragMode::ResizeTopLeft),
            (false, true, true, false) => Some(DragMode::ResizeTopRight),
            (true, false, false, true) => Some(DragMode::ResizeBottomLeft),
            (false, true, false, true) => Some(DragMode::ResizeBottomRight),
            (true, false, _, _) => Some(DragMode::ResizeLeft),
            (false, true, _, _) => Some(DragMode::ResizeRight),
            (_, _, true, false) => Some(DragMode::ResizeTop),
            (_, _, false, true) => Some(DragMode::ResizeBottom),
            _ => None,
        }
    }

    /// What cursor belongs at this point on the desktop.
    ///
    /// Resolution order, topmost window first: a resize border wins over
    /// everything (it is the compositor's own affordance, and a client cannot
    /// know the pointer is on it); then the client area shows whatever shape
    /// that window asked for; then the rest of the frame — title bar, buttons
    /// — is always an arrow, because those are the compositor's, not the
    /// client's. Off every window it is the desktop's arrow.
    ///
    /// A window's requested shape applies only where that window is, which is
    /// the whole reason the shape is stored per window: a client asking for an
    /// I-beam is describing its own text field, not seizing the desktop.
    fn cursor_at(&self, x: i32, y: i32) -> CursorShape {
        for &window_id in self.z_stack.iter().rev() {
            let Some(win) = self.window_ref(window_id) else {
                continue;
            };
            if !win.is_showing(self.current_workspace) {
                continue;
            }
            if let Some(mode) = self.detect_border_drag(win, x, y) {
                return match mode {
                    DragMode::ResizeLeft | DragMode::ResizeRight => CursorShape::ResizeEW,
                    DragMode::ResizeTop | DragMode::ResizeBottom => CursorShape::ResizeNS,
                    DragMode::ResizeTopLeft | DragMode::ResizeBottomRight => {
                        CursorShape::ResizeNWSE
                    }
                    DragMode::ResizeTopRight | DragMode::ResizeBottomLeft => {
                        CursorShape::ResizeNESW
                    }
                    DragMode::MoveWindow => CursorShape::Move,
                };
            }
            if win.client_rect().contains(x, y) {
                return win.cursor;
            }
            if win.outer_rect().contains(x, y) {
                // On the frame but not on a resize edge: the title bar and its
                // buttons are the compositor's furniture.
                return CursorShape::Arrow;
            }
        }
        // Over the desktop background.
        CursorShape::Arrow
    }

    /// Recompute the displayed cursor for a pointer at this position.
    fn update_cursor_shape(&mut self, x: i32, y: i32) {
        self.cursor_shape = self.cursor_at(x, y);
    }

    /// Bring every window's [`scale_factor`](Window::scale_factor) up to date
    /// with the display it currently sits on, damaging any window whose frame
    /// changed size as a result.
    ///
    /// # Why this is derived before use rather than maintained on move
    ///
    /// The obvious alternative is to set `scale_factor` at each of the places
    /// that move a window — `move_window`, maximize, restore, tile, snap,
    /// display hotplug. That is six sites today and seven the moment someone
    /// adds a window-placement feature, and a forgotten bump does not fail
    /// loudly: the window simply keeps the old monitor's decorations, which
    /// looks like a rendering bug a long way from the placement code that
    /// caused it. This is the same argument that made `route_window_list`
    /// compare bytes instead of counting epochs (design-decisions §494/§495) —
    /// a value recomputed before use has no site to forget.
    ///
    /// It is cheap enough to mean it: one rectangle intersection per window per
    /// display, on a list that is tens of entries long, once per frame — next
    /// to compositing millions of pixels it does not register.
    ///
    /// # Why the *client* rect decides the display
    ///
    /// Not [`outer_rect`](Window::outer_rect), which would be circular: the
    /// outer rect is the client rect plus decorations, decorations are sized by
    /// the scale, and the scale is what we are computing. Feeding the result
    /// back into its own input lets a window sitting astride a seam alternate
    /// between two scales forever, repainting on every frame. The client area
    /// is scale-independent — it is the client's own pixels — so it is a fixed
    /// point.
    ///
    /// # Why the comparison is on rectangles, not on the float
    ///
    /// What matters is whether anything moved, and a scale change too small to
    /// shift a pixel has not moved anything. Comparing the derived
    /// `outer_rect`s answers exactly that question, and sidesteps asking
    /// whether two `f32`s are "equal".
    pub fn refresh_window_scales(&mut self) {
        // Collected rather than damaged in place: `self.damage` and
        // `self.windows` are disjoint fields, but the borrow of `windows` is
        // held across the whole loop.
        let mut redraw: Vec<Rect> = Vec::new();
        for window in &mut self.windows {
            let scale = self.display_manager.scale_for(&window.client_rect());
            let before = window.outer_rect();
            window.scale_factor = scale;
            let after = window.outer_rect();
            if before != after {
                window.dirty = true;
                // Both: the frame vacated the old box and now occupies the new
                // one, and neither contains the other when the window shrank.
                redraw.push(before);
                redraw.push(after);
            }
        }
        for rect in redraw {
            self.damage.add(rect);
        }
    }

    // -----------------------------------------------------------------------
    // Compositing pipeline
    // -----------------------------------------------------------------------

    /// Composite a frame. Returns true if a frame was actually composited
    /// (false if skipped due to no damage or frame budget).
    pub fn compose_frame(&mut self) -> bool {
        // Before the damage check, not after: a window that moved onto a
        // higher-DPI display has grown, and the growth *is* the damage. Asking
        // "is there anything to draw?" first would answer no and leave the
        // frame at the old size until something else happened to dirty it.
        self.refresh_window_scales();

        // Check if we should compose (frame timing).
        if !self.frame_stats.should_compose() {
            return false;
        }

        // A frame carrying a drop preview is composited whole. The preview is a
        // *translucent* wash, so painting it over a partial frame would blend
        // it onto pixels that already carry last frame's copy of it, darkening
        // a little more every frame until it is opaque. Confining it to the
        // damage region instead would need a multi-rectangle frame clip the
        // render target does not have. This costs a full recomposite only
        // while the pointer is inside an edge band mid-drag, which is a
        // fraction of a second at a time.
        if self.drag_preview.is_some() {
            self.full_recomposite = true;
        }

        // Check if there's anything to composite.
        if !self.full_recomposite && !self.damage.has_damage() {
            return false;
        }

        self.frame_stats.begin_frame();

        // Fullscreen direct-scanout bypass: if the topmost window owns the whole
        // display with an opaque, display-sized shared buffer, present its
        // pixels straight from shared memory and skip compositing entirely —
        // no framebuffer clear, no per-pixel blit, no occluded windows drawn,
        // no buffer swap. The presented pixels come directly from the client
        // buffer (see `present_buffer`). This is the path games/video use.
        if let Some(wid) = self.direct_scanout_window() {
            self.scanout = Scanout::Direct(wid);
            // The compositor "consumed" the buffer for this frame; flag it for a
            // wl_buffer.release-style notification so the client may reuse it.
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == wid)
                && let Some(buf) = win.buffer.as_mut()
            {
                buf.mark_released();
            }
            self.full_recomposite = false;
            self.damage.clear();
            self.frame_stats.bypass_frames = self.frame_stats.bypass_frames.saturating_add(1);
            self.frame_stats.end_frame();
            return true;
        }
        self.scanout = Scanout::Composited;

        if self.full_recomposite {
            // Full recomposite: clear and redraw everything.
            self.full_recomposite_into_back();
        } else {
            // Partial recomposite: only redraw damaged areas.
            let damaged_rects: Vec<Rect> = self.damage.rects().to_vec();
            for rect in &damaged_rects {
                self.backend.clear_rect(rect, self.theme.desktop_background);
            }
            // Re-render windows that overlap with damaged areas.
            self.render_damaged_windows(&damaged_rects);
            self.damage.clear();
        }

        // After the windows and over them: the preview says where the window
        // is going, and a preview drawn under the windows it is about to
        // rearrange would be hidden by exactly the ones it is talking about.
        self.render_drag_preview();

        // Swap buffers.
        self.backend.present();

        self.frame_stats.end_frame();
        true
    }

    /// Full recomposite into the back buffer: clear to the desktop
    /// background and redraw every window bottom-to-top, then clear the
    /// pending-recomposite/damage state.
    ///
    /// Shared by [`compose_frame`](Compositor::compose_frame)'s
    /// full-recomposite branch and the benchmark hook
    /// [`bench_full_composite`](Compositor::bench_full_composite) so the two
    /// measure exactly the same work and can never drift. Does NOT swap
    /// buffers — the caller owns presentation.
    fn full_recomposite_into_back(&mut self) {
        // OPT (BENCH-COMPOSITOR-SLOW): don't clear the desktop background under
        // windows that will fully overwrite it with opaque content — that clear
        // is pure overdraw. `clear_except` fills only the uncovered region.
        let covered = self.opaque_cover_rects();
        self.backend
            .clear_except(self.theme.desktop_background, &covered);
        self.render_all_windows();
        self.full_recomposite = false;
        self.damage.clear();
    }

    /// Collect the screen-space rectangles that are guaranteed to be fully
    /// overwritten with opaque content during this recomposite.
    ///
    /// Used by [`full_recomposite_into_back`](Self::full_recomposite_into_back)
    /// to cull the desktop-background clear under opaque windows. Only windows
    /// whose *client area* is provably opaque and fully covered are included:
    ///
    /// - buffer-less windows whose first render command opaquely covers the
    ///   whole client area (same predicate the per-window bg-fill cull uses),
    ///   at full window opacity; and
    /// - buffer-backed windows carrying an opaque buffer at full opacity, over
    ///   the sub-rectangle actually covered by the buffer.
    ///
    /// Decorations (title bar, border, shadow) are deliberately excluded: they
    /// lie outside the client rect and the shadow is translucent, so the
    /// background under them must still be cleared. Being conservative here only
    /// costs a little extra (correct) overdraw, never correctness.
    fn opaque_cover_rects(&self) -> Vec<Rect> {
        self.windows
            .iter()
            .filter_map(|win| Self::window_opaque_cover(win, self.current_workspace))
            .collect()
    }

    /// The screen-space rectangle this one window is guaranteed to overwrite
    /// with fully opaque pixels, if any.
    ///
    /// Shared by the background-clear cull ([`opaque_cover_rects`](Self::opaque_cover_rects))
    /// and the inter-window cull in [`render_all_windows`](Self::render_all_windows),
    /// so the two can never disagree about what counts as opaque — a window
    /// treated as an occluder by one and not the other would leave a hole.
    fn window_opaque_cover(win: &Window, current_workspace: u32) -> Option<Rect> {
        if !win.is_showing(current_workspace) || win.opacity < 1.0 {
            return None;
        }
        if let Some(buf) = win.buffer.as_ref() {
            // Opaque shared buffer: covers min(buffer, client) from the client
            // origin.
            if !buf.is_opaque() {
                return None;
            }
            let cols = buf.width().min(win.width);
            let rows = buf.height().min(win.height);
            (cols > 0 && rows > 0).then(|| Rect::new(win.x, win.y, cols, rows))
        } else {
            Self::first_command_covers_client(
                &win.render_tree.commands,
                win.width,
                win.height,
                win.opacity,
            )
            .then(|| Rect::new(win.x, win.y, win.width, win.height))
        }
    }

    /// The screen-space rectangle a window can paint into, decorations included.
    ///
    /// Deliberately conservative — it is the *outer* bound of the shadow, so it
    /// over-covers rather than under-covers. Under-covering would clip a
    /// decoration off; over-covering only leaves a few culled pixels on the
    /// table.
    ///
    /// Derived from [`Window::frame_rect`] rather than re-spelled from the
    /// constants, which is what it used to do under a comment promising it
    /// "mirrors the geometry in `render_shadow`" — a promise nothing checked.
    /// The padding covers the furthest anything reaches: the shadow's last
    /// layer (one short of the shadow extent, cast 3 px down-right at 1×) and
    /// the border stroke (one out), with room to spare in every direction.
    ///
    /// All three terms are the window's *scaled* values, not the raw
    /// constants. A 2× window casts a 16 px shadow; culling it against an 8 px
    /// allowance would shave the outer half of the shadow off every window on
    /// a HiDPI display — and the symptom (a shadow that ends in a hard edge)
    /// looks nothing like the cause.
    fn window_drawn_extent(win: &Window) -> Rect {
        let (_, side, _) = win.frame_insets();
        win.frame_rect().inflate(
            win.shadow_extent()
                .saturating_add(side)
                .saturating_add(scale_dimension(3, win.scale_factor)),
        )
    }

    /// Benchmark/test hook: perform one full recomposite and buffer swap
    /// immediately, bypassing the vsync frame-rate gate that
    /// [`compose_frame`](Compositor::compose_frame) enforces.
    ///
    /// This exists so benchmarks can measure the raw composite cost
    /// deterministically (a tight loop over `compose_frame` would be
    /// throttled by `should_compose` and skip most iterations). It runs the
    /// same `full_recomposite_into_back` + `framebuffer.swap` sequence as the
    /// real full-recomposite path. Production code must call `compose_frame`,
    /// which honors vsync timing and the direct-scanout / partial-damage fast
    /// paths.
    #[doc(hidden)]
    pub fn bench_full_composite(&mut self) {
        self.full_recomposite = true;
        self.full_recomposite_into_back();
        self.backend.present();
    }

    /// Benchmark hook: one full recomposite, reporting the two phases apart.
    ///
    /// Returns `(background_clear_ns, window_render_ns)`. Same sequence as
    /// [`bench_full_composite`](Self::bench_full_composite) — it exists because
    /// the aggregate frame time cannot say which half to optimize, and the two
    /// halves have completely different fixes (memory bandwidth vs. overdraw).
    #[doc(hidden)]
    pub fn bench_full_composite_phases(&mut self) -> (u64, u64) {
        self.full_recomposite = true;
        let covered = self.opaque_cover_rects();
        let t0 = std::time::Instant::now();
        self.backend
            .clear_except(self.theme.desktop_background, &covered);
        let clear_ns = t0.elapsed().as_nanos() as u64;
        let t1 = std::time::Instant::now();
        self.render_all_windows();
        let windows_ns = t1.elapsed().as_nanos() as u64;
        self.full_recomposite = false;
        self.damage.clear();
        self.backend.present();
        (clear_ns, windows_ns)
    }

    /// Render all visible windows from bottom to top z-order, skipping the
    /// parts of each that windows above it will opaquely cover.
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): windows are painted back-to-front, so
    /// without this every pixel of every window is drawn even when a window
    /// above overwrites it a moment later. On the 4K benchmark's 16-window
    /// cascade each window is ~72% covered by its immediate successor alone,
    /// and window rendering was 11.1 ms of the 12.5 ms frame — so the hidden
    /// pixels, not the visible ones, were the bulk of the work.
    ///
    /// For each window this subtracts the opaque covers of every window above
    /// it from that window's drawn extent, and redraws it once per surviving
    /// fragment under [`Framebuffer::frame_clip`]. A window with nothing left
    /// is skipped outright. Correctness rests on two things: the occluders are
    /// only regions *provably* repainted opaquely later
    /// ([`window_opaque_cover`](Self::window_opaque_cover)), and the fragments
    /// are disjoint ([`Rect::subtract`]), so no pixel is painted twice.
    fn render_all_windows(&mut self) {
        /// Past this many fragments the per-fragment replay of a window's
        /// command list costs more than the pixels it saves.
        const MAX_FRAGMENTS: usize = 4;

        let z_stack_copy: Vec<WindowId> = self.z_stack.clone();

        if !self.occlusion_cull {
            for &window_id in &z_stack_copy {
                self.render_window(window_id);
            }
            return;
        }

        // Opaque cover per z-position, so window k can look at k+1.. without
        // re-deriving them for every window (O(n^2) predicate evaluations, and
        // `first_command_covers_client` walks a command list).
        let covers: Vec<Option<Rect>> = z_stack_copy
            .iter()
            .map(|&id| {
                self.window_ref(id)
                    .and_then(|win| Self::window_opaque_cover(win, self.current_workspace))
            })
            .collect();

        for (idx, &window_id) in z_stack_copy.iter().enumerate() {
            let Some(win) = self.window_ref(window_id) else {
                continue;
            };
            if !win.is_showing(self.current_workspace) {
                continue;
            }
            let extent = Self::window_drawn_extent(win);

            // Only occluders that actually meet this window matter; the rest
            // would just cost a subtraction that returns the input unchanged.
            let occluders: Vec<Rect> = covers
                .iter()
                .skip(idx.saturating_add(1))
                .flatten()
                .filter(|c| c.intersect(&extent).is_some())
                .copied()
                .collect();

            if occluders.is_empty() {
                self.render_window(window_id);
                continue;
            }

            match subtract_region(extent, &occluders, MAX_FRAGMENTS) {
                // Wholly hidden — the cheapest outcome there is.
                Some(parts) if parts.is_empty() => {}
                Some(parts) => {
                    for part in parts {
                        self.backend.set_frame_clip(Some(part));
                        self.render_window(window_id);
                    }
                    self.backend.set_frame_clip(None);
                }
                // Too fragmented to be worth it: draw it whole, as before.
                None => self.render_window(window_id),
            }
        }
    }

    /// Render only windows that overlap with the given damaged rects.
    fn render_damaged_windows(&mut self, damaged_rects: &[Rect]) {
        let z_stack_copy: Vec<WindowId> = self.z_stack.clone();
        for &window_id in &z_stack_copy {
            if let Some(win) = self.window_ref(window_id) {
                if !win.is_showing(self.current_workspace) {
                    continue;
                }
                let outer = win.outer_rect();
                let overlaps = damaged_rects.iter().any(|r| r.intersect(&outer).is_some());
                if overlaps {
                    self.render_window(window_id);
                }
            }
        }
    }

    /// Render a single window (shadow, decorations, client content).
    /// True when the client's first render command is an opaque, square-cornered
    /// `FillRect` that fully covers the client area and the window is fully
    /// opaque — meaning the compositor's default white background fill would be
    /// entirely painted over and can be skipped.
    ///
    /// Coordinates in render commands are client-local (origin at the client
    /// top-left), so a covering rect starts at or above/left of (0,0) and extends
    /// at least to `(win_width, win_height)`. Rounded corners are rejected because
    /// they would leave the corner pixels showing the background, and any window
    /// opacity < 1.0 is rejected because the top rect would then blend rather than
    /// fully replace the pixels beneath it.
    fn first_command_covers_client(
        commands: &[RenderCommand],
        win_width: u32,
        win_height: u32,
        opacity: f32,
    ) -> bool {
        if opacity < 1.0 {
            return false;
        }
        match commands.first() {
            Some(RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            }) => {
                color.a == 255
                    && *corner_radii == CornerRadii::ZERO
                    && *x <= 0.0
                    && *y <= 0.0
                    && *x + *width >= win_width as f32
                    && *y + *height >= win_height as f32
            }
            _ => false,
        }
    }

    fn render_window(&mut self, window_id: WindowId) {
        // Gather window data we need (avoiding borrow conflicts with self).
        let win_data = match self.window_ref(window_id) {
            Some(win) if win.is_showing(self.current_workspace) => (
                win.x,
                win.y,
                win.width,
                win.height,
                win.opacity,
                win.focused,
                win.title.clone(),
                win.render_tree.commands.clone(),
                win.buffer.is_some(),
                win.title_bar_layout(),
                win.transparent,
                win.maximized,
            ),
            _ => return,
        };

        let (
            win_x,
            win_y,
            win_width,
            win_height,
            opacity,
            focused,
            title,
            commands,
            has_buffer,
            title_bar,
            transparent,
            maximized,
        ) = win_data;

        // Undecorated and fullscreen windows get no frame: the first asked to
        // be a bare surface (a menu, a tooltip, a splash screen), the second
        // owns the whole display. Both report it by having no title bar.
        if let Some(bar) = title_bar {
            // 1. Draw window shadow — if the user wants shadows, and if this
            //    window has an edge to cast one from. A maximized window's frame
            //    is fitted to the display exactly (`maximize_window`), so every
            //    ring of its shadow is either clipped off the display or drawn
            //    under the window's own frame and painted over: on an opaque
            //    window it is invisible, and always it is a full shadow's worth
            //    of stroking per frame for nothing — on the one window state
            //    that is the common case. It is not merely wasted, either: a
            //    translucent or `transparent` window does not cover what is
            //    beneath it, so the rings show through as a dark smear along the
            //    top and left of a maximized window and nowhere else.
            //
            //    Fullscreen needs no test here — `Window::has_title_bar` is
            //    false for it, so a fullscreen window never reaches this branch.
            if self.appearance.drop_shadows && !maximized {
                self.render_shadow(bar.frame, bar.scale, opacity);
            }

            // 2. Draw window border.
            let border_color = if focused {
                self.theme.border_focused
            } else {
                self.theme.border_unfocused
            };
            self.render_border(bar.frame, bar.scale, border_color, opacity);

            // 3. Draw title bar.
            self.render_title_bar(&bar, focused, &title, opacity);
        }

        if has_buffer {
            // Shared-buffer (DMA-BUF) path: hand the client's pixels to the
            // backend as a textured quad. Disjoint field borrows: `windows` for
            // the buffer, `backend` for the destination — distinct fields, so
            // this is sound.
            //
            // The overlap of the buffer and the client area is resolved here
            // rather than below the seam: it is window geometry, which is the
            // compositor's business, not the rasterizer's.
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == window_id)
                && let Some(buf) = win.buffer.as_mut()
            {
                let cols = buf.width().min(win_width);
                let rows = buf.height().min(win_height);
                self.backend
                    .blit_buffer(buf, win_x, win_y, cols, rows, opacity);
                // The compositor is done reading this buffer for the frame;
                // flag it for a wl_buffer.release-style notification.
                buf.mark_released();
            }
        } else {
            // 4. Fill client area background (white) — UNLESS the client's first
            //    command already paints the whole client area opaquely, in which
            //    case the white fill is 100% overdraw. OPT (BENCH-COMPOSITOR-SLOW):
            //    skipping it removes a full-window fill per such window per frame
            //    (~29% of the 4K-benchmark's opaque stores). Only safe when the
            //    window itself is fully opaque (opacity >= 1.0) — otherwise the
            //    top rect blends and the background would show through.
            //
            //    A transparent window never gets the fill: the whole point of
            //    asking for one is that what the client does not paint shows
            //    the desktop through, and an opaque white undercoat is exactly
            //    what would prevent that.
            if !transparent
                && !Self::first_command_covers_client(&commands, win_width, win_height, opacity)
            {
                self.render_engine.fill_rect(
                    &mut self.backend,
                    win_x,
                    win_y,
                    win_width,
                    win_height,
                    0xFF_FF_FF_FF,
                    opacity,
                );
            }

            // 5. Execute client render commands.
            self.render_engine.execute(
                &mut self.backend,
                &commands,
                win_x,
                win_y,
                win_width,
                win_height,
                opacity,
            );
        }

        // Mark window as no longer dirty.
        if let Some(win) = self.window_mut(window_id) {
            win.dirty = false;
        }
    }

    /// Render the window shadow: concentric outlines around the frame box,
    /// offset down-right and fading with distance.
    ///
    /// Each ring is rounded by the window's own radius *grown by that ring's
    /// distance out*, which is what an offset curve actually is: a shadow whose
    /// rings all shared the frame's radius would be a stack of same-shaped
    /// outlines at increasing sizes, and its corners would bulge squarer the
    /// further out they went until the outermost ring poked past the curve it
    /// is supposed to be sitting behind.
    fn render_shadow(&mut self, frame: Rect, scale: f32, opacity: f32) {
        /// How far down and right the shadow is cast from the frame, at 1×.
        const SHADOW_OFFSET: u32 = 3;

        // Colour and peak alpha both from the palette rather than from a local
        // constant, so that the one place that says what a shadow looks like is
        // the same place that says what a title bar looks like. The alpha is
        // the innermost layer's; it falls off to nothing at the outermost.
        let shadow_rgb = self.theme.shadow_color & 0x00FF_FFFF;
        let shadow_alpha = self.theme.shadow_color >> 24;

        let extent = scale_dimension(SHADOW_SIZE, scale);
        let offset = scale_dimension(SHADOW_OFFSET, scale);
        // The falloff is derived from the layer count rather than being a
        // constant per-layer step, because the count now varies: a fixed step
        // of 5 over the 16 layers of a 2× shadow would reach zero alpha
        // half-way out and draw the outer half as nothing. Dividing the same
        // total alpha across however many layers there are keeps a 2× shadow a
        // scaled-up version of the 1× one instead of a truncated one. At the
        // unscaled extent of 8 this is 40/8 = 5, exactly the constant it
        // replaces.
        // `checked_div` rather than guarding the divisor: `scale_dimension`
        // already guarantees a non-zero extent for a non-zero constant, but a
        // guard is a second place that has to keep agreeing with the first. The
        // fallback value is unreachable anyway — a zero extent runs no layers.
        let falloff = shadow_alpha.checked_div(extent).unwrap_or(shadow_alpha);

        #[allow(
            clippy::cast_possible_wrap,
            reason = "a scaled 3px offset cannot approach i32::MAX"
        )]
        let base = frame.offset(offset as i32, offset as i32);
        let radius = self.decoration_radius(scale);
        for layer in 0..extent {
            let alpha = shadow_alpha
                .saturating_sub(layer.saturating_mul(falloff))
                .min(255);
            let ring = base.inflate(layer);
            #[allow(
                clippy::cast_precision_loss,
                reason = "the layer index is bounded by the scaled shadow extent \
                          — a handful of pixels, exact in f32"
            )]
            let grown = radius + layer as f32;
            let radii = CornerRadii::all(grown);
            // Only the outline of each layer: the interior is covered by the
            // window itself or by the next layer in.
            self.render_engine.stroke_round_rect(
                &mut self.backend,
                ring.x,
                ring.y,
                ring.width,
                ring.height,
                1,
                &radii,
                (alpha << 24) | shadow_rgb,
                opacity,
            );
        }
    }

    /// Render the window border: a stroke around the frame box.
    ///
    /// The stroke sits one border *above* the frame box, because
    /// [`Window::frame_insets`] reserves no room above the title bar for it —
    /// so the top edge is drawn into the shadow band rather than into space the
    /// layout set aside. Harmless (the band is 8 px of shadow) but a real
    /// inconsistency; see `known-issues.md`
    /// `TD-THE-TOP-BORDER-IS-DRAWN-OUTSIDE-THE-FRAME-INSETS`.
    fn render_border(&mut self, frame: Rect, scale: f32, color: u32, opacity: f32) {
        let width = scale_dimension(BORDER_WIDTH, scale);
        let border = Rect::new(
            frame.x,
            frame.y.saturating_sub(width as i32),
            frame.width,
            frame.height.saturating_add(width),
        );
        // The border traces the outside of the frame, so it takes the frame's
        // radius as-is — the same curve the title bar's top corners are drawn
        // with, from the same call, which is what keeps the two from parting
        // company by a pixel at the join.
        let radii = CornerRadii::all(self.decoration_radius(scale));
        self.render_engine.stroke_round_rect(
            &mut self.backend,
            border.x,
            border.y,
            border.width,
            border.height,
            width,
            &radii,
            color,
            opacity,
        );
    }

    /// Render the title bar with title text and buttons.
    ///
    /// The button rectangles are passed in rather than recomputed here: hit
    /// testing reads them from [`Window::close_button_rect`] and friends, and a
    /// second copy of the arithmetic is a button that is drawn in one place and
    /// clicked in another the moment either changes. `None` means the window
    /// does not have that button — a non-resizable window has no maximize.
    fn render_title_bar(&mut self, bar: &TitleBarLayout, focused: bool, title: &str, opacity: f32) {
        let tb_x = bar.bar.x;
        let tb_y = bar.bar.y;
        let tb_width = bar.bar.width;

        // Title bar background.
        let bg_color = if focused {
            self.theme.title_bar_focused
        } else {
            self.theme.title_bar_unfocused
        };
        // Rounded across the top only: the title bar shares its lower edge with
        // the client area, and curving that edge would cut two notches out of
        // the middle of the window where the bar meets the content beneath it.
        let radius = self.decoration_radius(bar.scale);
        self.render_engine.fill_round_rect(
            &mut self.backend,
            tb_x,
            tb_y,
            tb_width,
            bar.bar.height,
            &CornerRadii::top(radius),
            bg_color,
            opacity,
        );

        // Title text.
        let text_color = if focused {
            self.theme.title_text_focused
        } else {
            self.theme.title_text_unfocused
        };
        /// Gap between the left edge of the title bar and the title text, at 1×.
        const TITLE_TEXT_INSET: u32 = 8;
        let inset = scale_dimension(TITLE_TEXT_INSET, bar.scale);
        // The user's UI size, not a constant: a title bar is interface text,
        // and someone who enlarged the interface font because they cannot read
        // 13pt has said something about window titles too. Scaled on top of
        // that, because a title that stayed 13px inside a bar that grew with
        // the display is the visible half of an unscaled title bar — the frame
        // grows and the writing on it does not, which reads as a bug long
        // before anyone measures the pixels. `max` keeps a fractional scale, or
        // a font size a config file made tiny, from producing zero.
        let font_size = (self.appearance.fonts.ui_size * bar.scale).max(1.0);
        let text_x = tb_x.saturating_add(inset as i32);
        // Centred on the font's own line height rather than a hardcoded cell
        // size, so the title stays centred if the title-bar font ever changes.
        let line_height = self
            .render_engine
            .fonts
            .get(font_size, Weight::Regular, Family::Ui)
            .line_height();
        let text_y =
            tb_y.saturating_add((bar.bar.height as i32).saturating_sub(line_height as i32) / 2);
        // Reserve exactly the buttons this window actually has, so a window
        // with no maximize button gets that space for its title instead of
        // eliding text to make room for nothing. Measured from the drawn
        // rectangles rather than from the constants, so the reservation cannot
        // disagree with the buttons it is reserving for.
        let buttons = scale_dimension(TITLE_BUTTON_SIZE, bar.scale)
            .saturating_add(scale_dimension(TITLE_BUTTON_SPACING, bar.scale))
            .saturating_mul(bar.button_count());
        let max_text_width =
            tb_width.saturating_sub(buttons.saturating_add(inset.saturating_mul(2)));
        self.render_engine.draw_text(
            &mut self.backend,
            text_x,
            text_y,
            title,
            text_color,
            &[],
            opacity,
            Some(max_text_width),
            font_size,
            FontWeightHint::Regular,
            // A window title is chosen by the window, is as long as it likes,
            // and is the one string on screen a reader uses to tell two windows
            // apart. Cutting it without a mark is how "Save invoice-final" and
            // "Save invoice-final-2" become the same title.
            TextOverflow::Ellipsis,
        );

        // Buttons: close (red), maximize (green), minimize (yellow). Each is
        // drawn exactly where the hit test will look for it, and skipped
        // entirely when the window does not have it.
        //
        // The buttons are round when the windows are: a square close button
        // beside a curved corner is the mismatch, not the consistency. Capped at
        // half the button, which is the radius at which it becomes a circle —
        // past that the clamp inside the rasterizer would take over anyway, and
        // capping here means the three buttons agree with each other even when
        // they are not all the same size.
        let button_size = scale_dimension(TITLE_BUTTON_SIZE, bar.scale);
        #[allow(
            clippy::cast_precision_loss,
            reason = "a title-bar button is tens of pixels; exact in f32"
        )]
        let button_radius = radius.min(button_size as f32 / 2.0);
        let button_radii = CornerRadii::all(button_radius);
        for (rect, color) in [
            (bar.close, self.theme.close_button),
            (bar.maximize, self.theme.maximize_button),
            (bar.minimize, self.theme.minimize_button),
        ] {
            if let Some(r) = rect {
                self.render_engine.fill_round_rect(
                    &mut self.backend,
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    &button_radii,
                    color,
                    opacity,
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Protocol handling (stub IPC)
    // -----------------------------------------------------------------------

    /// Handle a compositor request from a client.
    pub fn handle_request(&mut self, request: CompositorRequest) -> CompositorResponse {
        match request {
            CompositorRequest::CreateWindow { spec, client_pid } => {
                let id = self.create_window_from_spec(&spec, client_pid);
                CompositorResponse::WindowCreated { window_id: id }
            }
            CompositorRequest::DestroyWindow { window_id } => {
                match self.destroy_window(window_id) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::SetTitle { window_id, title } => {
                match self.set_title(window_id, title) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Submit {
                window_id,
                commands,
            } => match self.submit_render(window_id, commands) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::Move { window_id, x, y } => {
                match self.move_window(window_id, x, y) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Resize {
                window_id,
                width,
                height,
            } => match self.resize_window(window_id, width, height) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::Minimize { window_id } => match self.minimize_window(window_id) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::Maximize { window_id } => match self.maximize_window(window_id) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::SetFullscreen { window_id, enable } => {
                match self.set_fullscreen(window_id, enable) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Restore { window_id } => match self.restore_window(window_id) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::SetVisible { window_id, visible } => {
                match self.set_visible(window_id, visible) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::SetCursor { window_id, cursor } => {
                match self.set_cursor(window_id, cursor) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::SetOpacity { window_id, opacity } => {
                match self.set_opacity(window_id, opacity) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::GetDisplayInfo => {
                if let Some(display) = self.display_manager.primary() {
                    CompositorResponse::DisplayInfo {
                        width: display.width,
                        height: display.height,
                        refresh_rate: display.refresh_rate,
                        scale_factor: display.scale_factor,
                    }
                } else {
                    CompositorResponse::Error {
                        message: "no primary display".to_string(),
                    }
                }
            }
            CompositorRequest::ReloadAppearance => {
                self.reload_appearance();
                // `Ok` whether or not anything changed. The client is being
                // told the compositor has re-read the file, which is true
                // either way, and a reply that differed would leak the state of
                // the user's settings to anyone allowed to ask for a reload.
                CompositorResponse::Ok
            }
            CompositorRequest::ReloadInput => {
                self.reload_input();
                // `Ok` whether or not anything changed, on the same terms as
                // the appearance reload above: a reply that differed would let
                // anyone allowed to ask for a reload read back the user's
                // settings from the shape of the answer.
                CompositorResponse::Ok
            }
            CompositorRequest::ShellControl { window_id, action } => {
                let result = match action {
                    ShellControlAction::Activate => self.activate_window(window_id),
                    ShellControlAction::Minimize => self.minimize_window(window_id),
                    ShellControlAction::Restore => self.restore_window(window_id),
                    ShellControlAction::Maximize => self.maximize_window(window_id),
                    ShellControlAction::Close => self.request_close(window_id),
                    ShellControlAction::SnapLeft => self.snap_window(window_id, SnapEdge::Left),
                    ShellControlAction::SnapRight => self.snap_window(window_id, SnapEdge::Right),
                    ShellControlAction::SnapToZone(slot) => {
                        self.snap_window_to_zone(window_id, slot)
                    }
                };
                match result {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::ReserveEdge {
                window_id,
                edge,
                size,
            } => match self.reserve_edge(window_id, edge, size) {
                Ok(area) => {
                    let rect = work_rect(area);
                    CompositorResponse::WorkArea {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    }
                }
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::SwitchWorkspace { workspace } => {
                self.switch_workspace(workspace);
                CompositorResponse::Ok
            }
            CompositorRequest::SetWindowWorkspace {
                window_id,
                workspace,
            } => match self.set_window_workspace(window_id, workspace) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::StreamStart => {
                let stream_id = self.start_stream();
                CompositorResponse::StreamStarted { stream_id }
            }
            CompositorRequest::StreamCapture { stream_id } => {
                match self.capture_stream(stream_id) {
                    Ok(data) => CompositorResponse::StreamFrame { data },
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::StreamStop { stream_id } => {
                if self.stop_stream(stream_id) {
                    CompositorResponse::Ok
                } else {
                    CompositorResponse::Error {
                        message: CompositorError::StreamNotFound(stream_id).to_string(),
                    }
                }
            }
        }
    }

    /// Drain pending event notifications in the compositor's own vocabulary.
    ///
    /// Prefer [`drain_input_frame`](Self::drain_input_frame) for anything
    /// actually being sent to a client; this stays for tests and for internal
    /// consumers that want the events before they are translated for the wire.
    pub fn drain_notifications(&mut self) -> Vec<EventNotification> {
        self.pending_notifications.drain(..).collect()
    }

    /// Drain pending notifications as an encoded `guiremote` input frame, ready
    /// to write to a client's transport.
    ///
    /// This is the seam that did not exist until now: the compositor has always
    /// hit-tested, tracked focus and built correctly addressed per-window
    /// events, and then had nowhere to put them, because the display protocol
    /// only ran outwards. See `known-issues.md` →
    /// `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`.
    ///
    /// Returns `None` when nothing is pending, so a caller can skip a write
    /// rather than send an empty frame every tick — input is bursty and most
    /// ticks have none.
    pub fn drain_input_frame(&mut self) -> Option<Vec<u8>> {
        if self.pending_notifications.is_empty() {
            return None;
        }
        let events: Vec<guiremote::InputEvent> = self
            .pending_notifications
            .drain(..)
            .map(wire_event)
            .collect();
        Some(guiremote::encode_input_frame(&events))
    }

    // -----------------------------------------------------------------------
    // Display management
    // -----------------------------------------------------------------------

    /// Handle a display resolution change.
    ///
    /// Everything on the desktop that was placed *by a rule* is re-derived
    /// against the new size, because a window stores the rectangle its rule
    /// produced and not the rule itself — the same fact that makes
    /// [`retile_for_work_area_change`](Self::retile_for_work_area_change)
    /// necessary when a taskbar changes height. A mode switch is that problem
    /// at its largest, and this used to do none of it: a maximised window kept
    /// its 1920-wide rectangle on a 1280-wide screen, with its right-hand third
    /// in pixels that no longer existed and its close button among them; a
    /// fullscreen client kept the old framebuffer's dimensions; and a window
    /// that happened to be sitting near the old bottom-right corner was left
    /// entirely off the new screen with no title bar on it to drag it back by.
    ///
    /// What is deliberately *not* re-derived is a window the user placed
    /// themselves and can still reach. A resize is not permission to re-lay-out
    /// the desktop, so only windows that would otherwise be unrecoverable are
    /// moved, and only by the smallest amount that recovers them.
    ///
    /// It is the **primary** display that is resized, and the scanout surface
    /// then follows the whole virtual desktop rather than that one screen —
    /// see [`resize_scanout_surface`](Self::resize_scanout_surface). On a
    /// one-monitor desktop those are the same rectangle, which is why this
    /// took a bare width and height for as long as there was only ever one.
    ///
    /// # Errors
    ///
    /// Whatever the backend's `resize` returns — an allocation failure for the
    /// new surface. Nothing else has been touched at that point, so the
    /// compositor carries on drawing the size it already had rather than
    /// half-adopting one it cannot paint.
    pub fn resize_display(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        // Asked before anything is mutated, so that a surface which cannot be
        // allocated leaves the display arrangement alone. `displays[0]` is the
        // primary by construction: `DisplayManager::new` creates it, and
        // `add_display` only ever appends beside it.
        let desktop = self
            .display_manager
            .virtual_bounds_if_resized(0, width, height);
        self.resize_scanout_surface(desktop)?;

        let Some(display) = self.display_manager.displays.first_mut() else {
            return Ok(());
        };
        display.width = width;
        display.height = height;
        let bounds = display.bounds();

        self.relayout_for_desktop_change(bounds);
        Ok(())
    }

    /// Add a monitor to the desktop, placed to the right of the ones already
    /// there, and grow the scanout surface to cover it.
    ///
    /// The counterpart of [`resize_display`](Self::resize_display) for the
    /// other way a desktop changes shape, and the reason
    /// [`DisplayManager::add_display`] is not simply exposed: adding a display
    /// enlarges the virtual desktop, and a virtual desktop larger than the
    /// surface it is composited into is one whose second monitor has no pixels
    /// behind it. Every window placed there would be drawn into a framebuffer
    /// that ends before it starts and clipped away entirely — the model would
    /// say the window was on the second screen and nothing would ever appear.
    ///
    /// # Errors
    ///
    /// As [`resize_display`](Self::resize_display): an allocation failure for
    /// the enlarged surface, with the display *not* added, so that the
    /// arrangement and the surface cannot come apart.
    pub fn attach_display(&mut self, display: Display) -> CompositorResult<()> {
        let mut manager = DisplayManager {
            displays: self.display_manager.displays.clone(),
        };
        manager.add_display(display);
        let desktop = manager.virtual_bounds();
        self.resize_scanout_surface(desktop)?;

        // Only now, when the surface it needs is known to exist.
        let bounds = manager
            .displays
            .last()
            .map_or_else(|| Rect::new(0, 0, 0, 0), Display::bounds);
        self.display_manager = manager;
        self.relayout_for_desktop_change(bounds);
        Ok(())
    }

    /// Give the display currently known as `from` the identity `to`.
    ///
    /// A compositor is built at a size before anything has told it *which*
    /// screen that size belongs to, so [`DisplayManager::new`] invents the id
    /// `0` for it. The scanout keys everything on the connector id — that is
    /// what [`Present::monitors`](present::Present::monitors) reports and what
    /// `Server::reconcile_monitors` matches the two sets on — so a desktop whose
    /// first screen is still called `0` has one monitor the reconciliation
    /// cannot recognise: it sees a connector it does not know (and attaches a
    /// second display for the *same* physical screen) and a display no connector
    /// claims (and detaches it). Both, once a second, for ever.
    ///
    /// So this exists to be called exactly once, at startup, by whoever knows
    /// what the first screen is actually plugged into. It is not a general
    /// renumbering facility: a display's id is the key windows are resolved
    /// against, and changing it under a running desktop is a different and much
    /// larger operation.
    ///
    /// # Errors
    ///
    /// [`CompositorError::DisplayError`] if no display is called `from`, or if
    /// one is already called `to` — two displays sharing an id would make
    /// [`DisplayManager::remove_display`] and every reconciliation ambiguous,
    /// silently, in a way that shows up as the wrong monitor going dark.
    pub fn rename_display(&mut self, from: u32, to: u32) -> CompositorResult<()> {
        if from == to {
            return Ok(());
        }
        if self.display_manager.displays.iter().any(|d| d.id == to) {
            return Err(CompositorError::DisplayError(format!(
                "cannot rename display {from} to {to}: a display already has that id"
            )));
        }
        let Some(display) = self
            .display_manager
            .displays
            .iter_mut()
            .find(|d| d.id == from)
        else {
            return Err(CompositorError::DisplayError(format!(
                "cannot rename display {from}: no display has that id"
            )));
        };
        display.id = to;
        Ok(())
    }

    /// Take a monitor off the desktop, shrink the scanout surface to what is
    /// left, and re-place everything the departed screen was holding.
    ///
    /// The mirror of [`attach_display`](Self::attach_display), and the half of
    /// monitor hotplug that does the work: a head that stops flipping is already
    /// dropped by the scanout, which keeps the *other* monitors alive, but until
    /// this is called the compositor still lists the display, still resolves
    /// windows onto it, and still composites a rectangle of frame that nothing
    /// copies out. A window maximised there is on a screen that no longer
    /// exists — visible nowhere, and unreachable, because a maximised window has
    /// no title bar edge left on any surviving monitor to drag it back by.
    ///
    /// Everything that re-places those windows is
    /// [`relayout_for_desktop_change`](Self::relayout_for_desktop_change),
    /// unchanged: with the display gone, `display_for` answers *primary* for any
    /// window that no longer overlaps a real monitor, so a maximised or snapped
    /// one is re-tiled onto the primary by the first pass, a fullscreen one is
    /// re-fitted by the second, a hand-placed one is rescued by the third and
    /// the pointer is pulled back by the fourth. That the removal case needed no
    /// new pass is a consequence of the rescue having been written against the
    /// virtual desktop rather than against one screen.
    ///
    /// **The order is the reverse of `attach_display`'s, on purpose.** Attaching
    /// allocates the larger surface *first* and adopts the display only if that
    /// succeeded, because a desktop wider than its framebuffer has a monitor
    /// with no pixels behind it. Detaching adopts first and shrinks after,
    /// because the monitor is already physically gone: refusing to acknowledge
    /// that would leave the model describing a screen that does not exist, which
    /// is the bug, whereas a surface that stays too large still covers the
    /// desktop completely. A failed shrink wastes memory; it cannot draw
    /// anything wrong.
    ///
    /// # Errors
    ///
    /// [`CompositorError::DisplayError`] if `id` names no attached display, or
    /// if it names the last one. A desktop with no monitors has zero-sized
    /// virtual bounds, no primary to fall back to, and every window on it
    /// stranded with nowhere to be rescued to; there is no arrangement to adopt,
    /// so the last monitor is kept and the caller told. A display server whose
    /// only screen has been unplugged has nothing useful left to do either way.
    pub fn detach_display(&mut self, id: u32) -> CompositorResult<()> {
        if self.display_manager.displays.len() <= 1 {
            return Err(CompositorError::DisplayError(format!(
                "cannot detach display {id}: it is the only monitor left"
            )));
        }
        if self.display_manager.remove_display(id).is_none() {
            return Err(CompositorError::DisplayError(format!(
                "cannot detach display {id}: no display has that id"
            )));
        }

        let desktop = self.display_manager.virtual_bounds();
        if self.resize_scanout_surface(desktop).is_err() {
            // Handled rather than propagated, for the reason in the doc comment
            // above: the arrangement has already changed and cannot be put back.
            // The surface keeps the size it had, which still covers the smaller
            // desktop, but it also still holds the departed monitor's last
            // frame, so the full clear the successful path would have done has
            // to happen here too or that picture stays on screen.
            let (width, height) = self.backend.size();
            self.full_recomposite = true;
            self.damage.mark_full(width, height);
        }

        // The screen a stranded window is put on has to be one that exists, and
        // after a removal the only one guaranteed to is the primary — which
        // `remove_display` has just promoted if the departed monitor was it.
        let home = self
            .display_manager
            .primary()
            .map_or(desktop, Display::bounds);
        self.relayout_for_desktop_change(home);
        Ok(())
    }

    /// Grow or shrink the composited surface to hold `desktop`.
    ///
    /// **The surface is the virtual desktop, not one monitor.** A second
    /// monitor is a second *viewport* onto one composed frame rather than a
    /// second frame: windows straddling the seam are one rectangle in one
    /// buffer and each head copies out its own part, so nothing in the
    /// compositing pipeline has to learn what a head is. What does learn it is
    /// the scanout, which owns a buffer pair per head and blits each head's
    /// rectangle out of this frame.
    ///
    /// The cost, stated plainly: a monitor arrangement whose bounding box is
    /// larger than the monitors themselves — an L-shape, or two screens offset
    /// vertically — composites the gap and scans it out nowhere. That is
    /// bounded by the arrangement the user chose and is paid in a clear, not in
    /// per-window work.
    ///
    /// The surface's pixel (0, 0) is the virtual desktop's (0, 0), so a display
    /// at a negative offset would be unaddressable. Nothing produces one:
    /// [`DisplayManager::add_display`] only ever places to the right of what is
    /// already there. `right()`/`bottom()` rather than `width`/`height` for the
    /// same reason — they are the extent that has to exist, and they agree with
    /// the size only while the origin is where it is documented to be.
    fn resize_scanout_surface(&mut self, desktop: Rect) -> CompositorResult<()> {
        let width = u32::try_from(desktop.right().max(0)).unwrap_or(u32::MAX);
        let height = u32::try_from(desktop.bottom().max(0)).unwrap_or(u32::MAX);
        self.backend.resize(width, height)?;
        self.full_recomposite = true;
        self.damage.mark_full(width, height);
        Ok(())
    }

    /// Put the desktop back in order after one monitor changed shape or arrived.
    ///
    /// `changed` is that monitor's new bounds — the one whose tiled windows must
    /// be re-derived, and the screen a window that ends up nowhere is put on.
    ///
    /// The first two passes cannot fight over a window that is both maximised
    /// and fullscreen — the re-tile excludes fullscreen outright — so their
    /// order is free. The rescue must follow both, because a window placed by
    /// either rule is by definition not stranded and asking before they have run
    /// would move one that was about to be re-placed anyway.
    fn relayout_for_desktop_change(&mut self, changed: Rect) {
        self.retile_for_work_area_change(changed);
        self.refit_fullscreen_windows();
        self.bring_stranded_windows_back(changed);
        self.pull_pointer_onto_the_desktop();
    }

    /// Re-fit every fullscreen window to the monitor it is on, after one of them
    /// changed size.
    ///
    /// Fullscreen is defined against a whole *monitor* rather than against a
    /// work area — covering the panels is the point, and it is what makes such
    /// a window eligible for the direct-scanout bypass in
    /// [`compose_frame`](Self::compose_frame) on a single-head desktop — so it
    /// is re-derived here rather than by the re-tile, which divides up work
    /// areas and would hand it back the taskbar's leftovers.
    ///
    /// Each window is re-fitted to *its own* screen, resolved the same way
    /// [`set_fullscreen`] resolves it, rather than all of them to the one that
    /// changed. Handing the second monitor's dimensions to a game fullscreen on
    /// the first is the same class of error as sizing it from the framebuffer,
    /// and every fullscreen window whose screen did not move falls out as a
    /// no-op rather than needing to be excluded.
    ///
    /// The client is told, for the same reason [`set_fullscreen`] tells it: its
    /// surface is now a different size, and it is the only party that can
    /// redraw at it.
    ///
    /// [`set_fullscreen`]: Self::set_fullscreen
    fn refit_fullscreen_windows(&mut self) {
        let affected: Vec<(WindowId, Rect)> = self
            .windows
            .iter()
            .filter(|w| w.fullscreen)
            .map(|w| (w.id, self.work_bounds_for(w.frame_rect())))
            // Nothing to say to a client whose surface did not move or change
            // size — a spurious `WindowResized` is a repaint it did not need.
            // This is also what keeps a mode change on one monitor silent for
            // the fullscreen windows on all the others.
            .filter(|&(id, screen)| {
                self.window_ref(id)
                    .is_some_and(|w| w.client_rect() != screen)
            })
            .collect();

        for (window_id, screen) in affected {
            self.damage_window(window_id);
            if let Some(window) = self.window_mut(window_id) {
                window.x = screen.x;
                window.y = screen.y;
                window.width = screen.width;
                window.height = screen.height;
                window.dirty = true;
            }
            self.damage_window(window_id);
            self.pending_notifications
                .push_back(EventNotification::WindowResized {
                    window_id,
                    width: screen.width,
                    height: screen.height,
                });
        }
    }

    /// Move back any window the new display size left entirely off the desktop.
    ///
    /// *Stranded* is judged against the whole virtual desktop and not against
    /// `bounds`, so that shrinking one monitor does not evacuate the others: a
    /// window living on the second screen is untouched by a mode change on the
    /// first, and dragging it across would be a far more visible bug than the
    /// one being fixed. `bounds` — the display that just changed — is only the
    /// place a genuinely stranded window is put, because it is a screen that is
    /// known to exist and is the one most likely to have stranded it.
    ///
    /// Tiled and fullscreen windows are excluded because their geometry was
    /// already re-derived from the new size by the two passes before this one: a
    /// window that follows a rule cannot be stranded by the rule moving.
    ///
    /// See [`kept_reachable`] for why the intersection test is as strict as it
    /// is.
    fn bring_stranded_windows_back(&mut self, bounds: Rect) {
        let desktop = self.display_manager.virtual_bounds();
        let moves: Vec<(WindowId, i32, i32)> = self
            .windows
            .iter()
            .filter(|w| !w.fullscreen && !w.maximized && w.snapped.is_none())
            .map(|w| (w, kept_reachable(w.frame_rect(), desktop, bounds)))
            .filter(|&(w, placed)| placed != w.frame_rect())
            .map(|(w, placed)| {
                // Through the frame rather than the client area: it is the
                // decorated box that has to land on screen, and `pulled_onto`
                // pins its top-left — where the title bar is — when it is too
                // large to fit.
                let (x, y, _, _) = w.client_geometry_for_frame(placed);
                (w.id, x, y)
            })
            .collect();

        for (window_id, x, y) in moves {
            // `WindowNotFound` is unreachable: every id came from the list above
            // and nothing between there and here removes a window. Dropped
            // rather than propagated because a caller adopting a new display
            // mode has no answer to "one window declined to move" and must not
            // abandon the rest of the resize over it.
            drop(self.move_window(window_id, x, y));
        }
    }

    /// Bring the pointer inside the desktop it may have just fallen off.
    ///
    /// The cursor position is not derived from anything — it is whatever the
    /// last motion event said — so a screen that shrinks under it leaves it at a
    /// coordinate the display no longer has: an invisible pointer that
    /// hit-tests against nothing, and stays that way until the user moves the
    /// mouse and the input source volunteers a fresh position.
    ///
    /// Clamped to the *virtual* bounds rather than to one monitor because the
    /// pointer crosses monitors and the desktop is their union; clamping it to
    /// the primary would teleport it off a second screen that a resize of the
    /// first did not touch.
    fn pull_pointer_onto_the_desktop(&mut self) {
        let bounds = self.display_manager.virtual_bounds();
        if bounds.width == 0 || bounds.height == 0 {
            // No desktop to be on. The clamp below would also be malformed —
            // its lower bound would exceed its upper — which panics.
            return;
        }
        self.cursor_x = self
            .cursor_x
            .clamp(bounds.x, bounds.right().saturating_sub(1));
        self.cursor_y = self
            .cursor_y
            .clamp(bounds.y, bounds.bottom().saturating_sub(1));
    }

    /// Get the display manager.
    pub fn display_manager(&self) -> &DisplayManager {
        &self.display_manager
    }

    /// Get frame statistics.
    pub fn frame_stats(&self) -> &FrameStats {
        &self.frame_stats
    }

    /// Get the current cursor shape.
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// Where the pointer is, in screen pixels.
    ///
    /// The cheapest evidence that input reached the compositor at all: a caller
    /// that fed a [`InputEvent::MouseMove`] can check it landed without
    /// composing a frame and hunting for a cursor in the pixels.
    #[must_use]
    pub const fn cursor_position(&self) -> (i32, i32) {
        (self.cursor_x, self.cursor_y)
    }

    /// Get a reference to the framebuffer's front buffer (the composited
    /// surface). Note: when the last frame was a direct-scanout bypass this is
    /// *stale* — use [`present_pixels`](Compositor::present_pixels) for the
    /// pixels actually being displayed.
    pub fn front_buffer(&self) -> &[u32] {
        self.backend.presented_pixels()
    }

    /// Get the pixels actually being presented to the display this frame.
    ///
    /// For a composited frame this is the framebuffer front buffer; for a
    /// fullscreen direct-scanout bypass it is the client's shared-buffer pixels
    /// referenced directly (zero copy). Falls back to the front buffer if the
    /// scanned-out window/buffer vanished between compose and present.
    #[must_use]
    pub fn present_pixels(&self) -> &[u32] {
        if let Scanout::Direct(wid) = self.scanout
            && let Some(win) = self.window_ref(wid)
            && let Some(buf) = win.buffer.as_ref()
        {
            return buf.pixels();
        }
        self.backend.presented_pixels()
    }

    /// The size of a composited frame, in pixels, as `(width, height)`.
    ///
    /// [`present_pixels`](Compositor::present_pixels) returns `width * height`
    /// values and says nothing about their shape; anything drawing them on a
    /// display needs both, and asking the primary [`Display`] instead would be
    /// the wrong question — a display describes the *hardware* mode, and the
    /// framebuffer is what was actually composited into.
    #[must_use]
    pub const fn frame_size(&self) -> (u32, u32) {
        self.backend.size()
    }

    /// How the last presented frame was produced.
    #[must_use]
    pub fn scanout(&self) -> Scanout {
        self.scanout
    }

    /// Whether the last presented frame used direct-scanout bypass.
    #[must_use]
    pub fn is_scanout_bypassed(&self) -> bool {
        matches!(self.scanout, Scanout::Direct(_))
    }

    /// Get the number of managed windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Find the index of a window by ID.
    fn window_index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// Get a reference to a window by ID.
    fn window_ref(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Get a mutable reference to a window by ID.
    fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The band a window sits in, or [`Layer::Normal`] if there is no such
    /// window.
    ///
    /// The fallback is unreachable for any id that is in `z_stack`, which is
    /// the only place this is called from — every id there names a live
    /// window. It exists because returning a `Layer` rather than an
    /// `Option<Layer>` keeps [`Self::stack_insertion_index`] a straight count.
    fn layer_of(&self, id: WindowId) -> Layer {
        self.window_ref(id).map_or(Layer::Normal, |w| w.layer)
    }

    /// Where in `z_stack` a window of `layer` goes when it is raised to the top
    /// of its own band.
    ///
    /// `z_stack` is kept partitioned by band, ascending, so the insertion point
    /// is simply the number of windows in bands at or below `layer` — after all
    /// of them, before the first window of any higher band. That partitioning
    /// is the invariant this whole layering rests on, and it is maintained by
    /// this function being the *only* way anything enters the stack.
    fn stack_insertion_index(&self, layer: Layer) -> usize {
        self.z_stack
            .iter()
            .filter(|&&id| self.layer_of(id) <= layer)
            .count()
    }

    /// Put `id` at the top of its own band, removing it from wherever it was.
    fn raise_within_layer(&mut self, id: WindowId) {
        let layer = self.layer_of(id);
        self.z_stack.retain(|&other| other != id);
        let at = self.stack_insertion_index(layer);
        self.z_stack.insert(at, id);
        self.update_z_orders();
    }

    /// The desktop's windows as a shell sees them, bottom-to-top.
    ///
    /// The whole set, including invisible and minimized windows and every
    /// stacking band, because filtering is the *shell's* decision and it needs
    /// different subsets for different jobs: a taskbar wants the minimized ones
    /// (that is what its buttons restore) and not the overlays; an Alt-Tab
    /// switcher wants neither the minimized ones nor the background. A
    /// compositor that pre-filtered would have to pick one of those and be
    /// wrong for the other, with no way for the client to recover what was
    /// dropped.
    ///
    /// Ordered by `z_stack` rather than by creation, so consecutive entries are
    /// neighbours on screen and the last one is the topmost window.
    ///
    /// Windows on other virtual desktops are in it, carrying their own desktop
    /// number, and the list says which desktop is showing. Leaving them out
    /// would be the original bug back in a new place: a switcher that could only
    /// offer this desktop's windows makes the others unreachable.
    #[must_use]
    pub fn window_list(&self) -> WindowList {
        WindowList::new(
            self.current_workspace,
            self.z_stack
                .iter()
                .filter_map(|&id| self.window_ref(id))
                .map(|w| WindowInfo {
                    id: w.id.raw(),
                    pid: w.client_pid,
                    layer: w.layer,
                    title: w.title.clone(),
                    visible: w.visible,
                    minimized: w.minimized,
                    maximized: w.maximized,
                    // From the window's own flag rather than from
                    // `self.focused_window`, so the list cannot disagree with
                    // the window it describes if the two ever drift apart.
                    focused: w.focused,
                    workspace: w.workspace,
                    // Reported, never accepted back. Nothing in `ShellControl`
                    // takes a rectangle — a snap names an edge (§505) and a
                    // maximize names nothing — so this is something a shell
                    // draws with, not something it can move a window by. That
                    // is the whole difference between this and the per-window
                    // geometry §506 deleted from the shell's own list.
                    x: w.x,
                    y: w.y,
                    width: w.width,
                    height: w.height,
                })
                .collect(),
        )
    }

    /// Update z_order fields on all windows based on their position in z_stack.
    fn update_z_orders(&mut self) {
        let z_stack_copy = self.z_stack.clone();
        for (i, &id) in z_stack_copy.iter().enumerate() {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                win.z_order = i as u32;
            }
        }
    }

    /// Capture the current scene as a draw-command stream frame for a remote
    /// viewer (native compositor-level streaming).
    ///
    /// Walks the z-stack bottom-to-top, includes every visible, non-minimized
    /// window, and hands the per-window render-command lists to `session`,
    /// which forwards only the commands that changed since the last frame
    /// (geometry-only deltas otherwise). The buffer (DMA-BUF) path has no
    /// vector commands to forward, so such windows stream as empty command
    /// lists — pixel forwarding for those is the video-encoded fallback's job,
    /// not this path's.
    pub fn capture_stream_frame(&self, session: &mut SceneSession) -> SceneFrame {
        let mut snaps: Vec<WindowSnapshot<'_>> = Vec::with_capacity(self.z_stack.len());
        for &id in &self.z_stack {
            if let Some(win) = self.window_ref(id) {
                if !win.is_showing(self.current_workspace) {
                    continue;
                }
                snaps.push(WindowSnapshot {
                    id: win.id.raw(),
                    x: win.x,
                    y: win.y,
                    width: win.width,
                    height: win.height,
                    opacity: win.opacity,
                    commands: &win.render_tree,
                });
            }
        }
        let (fb_w, fb_h) = self.backend.size();
        session.build_frame(fb_w, fb_h, &snaps)
    }

    /// Begin a remote draw-command stream session and return its id. A remote
    /// desktop service calls this once per connected viewer, then polls
    /// [`capture_stream`](Self::capture_stream) each frame.
    pub fn start_stream(&mut self) -> u64 {
        let id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.wrapping_add(1);
        self.stream_sessions.insert(id, SceneSession::new());
        id
    }

    /// Capture the current scene for stream `stream_id` and return the encoded
    /// wire frame (geometry-only deltas for unchanged windows). Errors if the
    /// id is unknown (e.g. the session was already stopped).
    pub fn capture_stream(&mut self, stream_id: u64) -> CompositorResult<Vec<u8>> {
        // Take ownership of the session so capture_stream_frame can borrow
        // &self immutably while mutating the (now-local) session; reinsert after.
        let mut session = self
            .stream_sessions
            .remove(&stream_id)
            .ok_or(CompositorError::StreamNotFound(stream_id))?;
        let frame = self.capture_stream_frame(&mut session);
        let bytes = guiremote::scene::encode_scene_frame(&frame);
        self.stream_sessions.insert(stream_id, session);
        Ok(bytes)
    }

    /// Stop a stream session, freeing its delta-tracking state. Returns whether
    /// a session with that id existed.
    pub fn stop_stream(&mut self, stream_id: u64) -> bool {
        self.stream_sessions.remove(&stream_id).is_some()
    }

    /// Number of active stream sessions (for diagnostics/tests).
    #[must_use]
    pub fn stream_session_count(&self) -> usize {
        self.stream_sessions.len()
    }

    /// Focus the topmost visible window.
    fn focus_topmost_visible(&mut self) {
        let topmost = self.z_stack.iter().rev().copied().find(|&id| {
            self.window_ref(id)
                .is_some_and(|w| w.is_showing(self.current_workspace))
        });

        if let Some(id) = topmost {
            self.focus_window(id);
        }
    }

    /// Mark the area occupied by a window (including decorations) as damaged.
    fn damage_window(&mut self, window_id: WindowId) {
        if let Some(win) = self.window_ref(window_id) {
            let outer = win.outer_rect();
            self.damage.add(outer);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// The five defensive lints the workspace turns on are for production code:
// a test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and rewriting that assertion as a `let else`
// only hides which line failed. CLAUDE.md's lint policy says as much.
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    /// One pixel of the frame currently being composited, by `(x, y)`.
    ///
    /// The backend-agnostic form of `Framebuffer::get_pixel`: a test that
    /// inspects a composite before it is presented has to ask the *backend*,
    /// because a GPU one has no `Framebuffer` to reach into. Returns `None`
    /// outside the surface, as `get_pixel` did.
    fn working_pixel(backend: &RenderBackend, x: u32, y: u32) -> Option<u32> {
        let (w, h) = backend.size();
        if x >= w || y >= h {
            return None;
        }
        backend
            .working_pixels()
            .get(Framebuffer::pixel_index(w as usize, x as usize, y as usize))
            .copied()
    }

    /// What `main` used to do, kept because it is a compact tour of the API:
    /// a window, a picture in it, and a composited frame.
    #[test]
    fn the_demo_scene_still_composites() {
        let mut compositor = Compositor::new(1920, 1080, 60).expect("compositor");
        let window_id = compositor.create_window("Welcome to Slate OS".to_string(), 640, 480, 1);

        let mut tree = RenderTree::new();
        tree.fill_rect(10.0, 10.0, 200.0, 40.0, Color::BLUE);
        tree.text(
            20.0,
            20.0,
            "Hello from Slate OS Compositor!",
            Color::WHITE,
            14.0,
        );
        tree.fill_rect(10.0, 60.0, 620.0, 1.0, Color::LIGHT_GRAY);
        compositor
            .submit_render(window_id, tree.commands)
            .expect("submit");

        assert!(compositor.compose_frame(), "nothing was drawn");
        assert_eq!(compositor.window_count(), 1);
        assert!(
            compositor.frame_stats().last_frame_time_us > 0,
            "the frame took no measurable time, so it did no work"
        );
    }

    #[test]
    fn test_window_id_uniqueness() {
        let id1 = WindowId::allocate();
        let id2 = WindowId::allocate();
        let id3 = WindowId::allocate();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 20, 100, 50);
        assert!(rect.contains(10, 20));
        assert!(rect.contains(50, 40));
        assert!(rect.contains(109, 69));
        assert!(!rect.contains(110, 70));
        assert!(!rect.contains(9, 20));
        assert!(!rect.contains(10, 19));
    }

    #[test]
    fn test_rect_intersect() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let intersection = a.intersect(&b);
        assert!(intersection.is_some());
        let i = intersection.unwrap();
        assert_eq!(i.x, 50);
        assert_eq!(i.y, 50);
        assert_eq!(i.width, 50);
        assert_eq!(i.height, 50);

        // Non-overlapping
        let c = Rect::new(200, 200, 50, 50);
        assert!(a.intersect(&c).is_none());
    }

    #[test]
    fn test_rect_union() {
        let a = Rect::new(10, 10, 50, 50);
        let b = Rect::new(40, 40, 80, 80);
        let u = a.union(&b);
        assert_eq!(u.x, 10);
        assert_eq!(u.y, 10);
        assert_eq!(u.width, 110);
        assert_eq!(u.height, 110);
    }

    #[test]
    fn test_framebuffer_creation() {
        let fb = Framebuffer::new(800, 600);
        assert!(fb.is_ok());
        let fb = fb.unwrap();
        assert_eq!(fb.width, 800);
        assert_eq!(fb.height, 600);
    }

    #[test]
    fn test_framebuffer_invalid_dimensions() {
        assert!(Framebuffer::new(0, 100).is_err());
        assert!(Framebuffer::new(100, 0).is_err());
        assert!(Framebuffer::new(MAX_FB_WIDTH + 1, 100).is_err());
    }

    #[test]
    fn test_framebuffer_pixel_ops() {
        let mut fb = Framebuffer::new(100, 100).unwrap();
        fb.set_pixel(50, 50, 0xFF_FF_00_00);
        assert_eq!(fb.get_pixel(50, 50), Some(0xFF_FF_00_00));
        assert_eq!(fb.get_pixel(200, 200), None); // Out of bounds
    }

    #[test]
    fn test_framebuffer_swap() {
        let mut fb = Framebuffer::new(10, 10).unwrap();
        fb.set_pixel(0, 0, 0xFF_11_22_33);
        fb.swap();
        // After swap, front buffer should have the pixel.
        assert_eq!(fb.front_buffer()[0], 0xFF_11_22_33);
        // Back buffer should be the old front (initial black).
        assert_eq!(fb.get_pixel(0, 0), Some(0xFF_00_00_00));
    }

    #[test]
    fn test_compositor_creation() {
        let comp = Compositor::new(1920, 1080, 60);
        assert!(comp.is_ok());
    }

    #[test]
    fn test_create_and_destroy_window() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 42);
        assert_eq!(comp.window_count(), 1);
        assert_eq!(comp.focused_window, Some(id));

        assert!(comp.destroy_window(id).is_ok());
        assert_eq!(comp.window_count(), 0);
        assert_eq!(comp.focused_window, None);
    }

    #[test]
    fn test_window_focus() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id1 = comp.create_window("Win1".to_string(), 300, 200, 1);
        let id2 = comp.create_window("Win2".to_string(), 300, 200, 2);

        // Second window should be focused.
        assert_eq!(comp.focused_window, Some(id2));

        // Focus the first window.
        comp.focus_window(id1);
        assert_eq!(comp.focused_window, Some(id1));

        // First window should be on top of z-stack.
        assert_eq!(comp.z_stack.last(), Some(&id1));
    }

    #[test]
    fn test_window_minimize_restore() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 1);

        assert!(comp.minimize_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(win.minimized);
        assert!(!win.visible);

        assert!(comp.restore_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(!win.minimized);
        assert!(win.visible);
    }

    #[test]
    fn test_window_maximize_restore() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 1);

        let original_width = comp.window_ref(id).unwrap().width;
        assert!(comp.maximize_window(id).is_ok());

        let win = comp.window_ref(id).unwrap();
        assert!(win.maximized);
        assert!(win.width > original_width);

        assert!(comp.restore_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(!win.maximized);
        assert_eq!(win.width, original_width);
    }

    #[test]
    fn test_damage_region() {
        let mut damage = DamageRegion::new();
        assert!(!damage.has_damage());

        damage.add(Rect::new(10, 10, 50, 50));
        assert!(damage.has_damage());
        assert_eq!(damage.rects().len(), 1);

        // Add non-overlapping rect.
        damage.add(Rect::new(200, 200, 30, 30));
        assert_eq!(damage.rects().len(), 2);

        damage.clear();
        assert!(!damage.has_damage());
    }

    #[test]
    fn test_damage_region_merge() {
        let mut damage = DamageRegion::new();
        damage.add(Rect::new(10, 10, 50, 50));
        // Overlapping rect should merge.
        damage.add(Rect::new(30, 30, 50, 50));
        assert_eq!(damage.rects().len(), 1);
        // Merged rect should be the union.
        let merged = &damage.rects()[0];
        assert_eq!(merged.x, 10);
        assert_eq!(merged.y, 10);
        assert_eq!(merged.width, 70);
        assert_eq!(merged.height, 70);
    }

    #[test]
    fn test_compositor_request_response() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();

        let resp = comp.handle_request(CompositorRequest::CreateWindow {
            spec: WindowSpec::new("Protocol Test", 320, 240),
            client_pid: 99,
        });
        let window_id = match resp {
            CompositorResponse::WindowCreated { window_id } => window_id,
            _ => panic!("expected WindowCreated response"),
        };

        // Set title.
        let resp = comp.handle_request(CompositorRequest::SetTitle {
            window_id,
            title: "Renamed".to_string(),
        });
        assert!(matches!(resp, CompositorResponse::Ok));
        assert_eq!(comp.window_ref(window_id).unwrap().title, "Renamed");

        // Get display info.
        let resp = comp.handle_request(CompositorRequest::GetDisplayInfo);
        match resp {
            CompositorResponse::DisplayInfo {
                width,
                height,
                refresh_rate,
                ..
            } => {
                assert_eq!(width, 800);
                assert_eq!(height, 600);
                assert_eq!(refresh_rate, 60);
            }
            _ => panic!("expected DisplayInfo response"),
        }
    }

    #[test]
    fn test_hit_testing() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id1 = comp.create_window("Win1".to_string(), 200, 150, 1);
        let _id2 = comp.create_window("Win2".to_string(), 200, 150, 2);

        // Windows are placed at offset positions. The first window is at (100, 80).
        let win1 = comp.window_ref(id1).unwrap();
        let center_x = win1.x + win1.width as i32 / 2;
        let center_y = win1.y + win1.height as i32 / 2;

        // Win1 is below Win2 in z-order, but if we click in its non-overlapping area,
        // we should hit it.
        let hit = comp.window_at(center_x, center_y);
        // This could be id1 or id2 depending on overlap. Just verify we get something.
        assert!(hit.is_some());
    }

    #[test]
    fn test_compose_frame() {
        let mut comp = Compositor::new(320, 240, 60).unwrap();
        comp.create_window("Test".to_string(), 200, 150, 1);

        // First compose should work (full_recomposite is set).
        assert!(comp.compose_frame());

        // Second compose without damage should not compose.
        // (Need to wait for frame interval, but in tests frame_stats starts fresh.)
        // Force frame timing to allow immediate recompose.
        comp.frame_stats.last_frame_start = None;
        assert!(!comp.compose_frame()); // No damage.
    }

    #[test]
    fn test_input_routing_keyboard() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Focused".to_string(), 400, 300, 1);

        comp.handle_input(InputEvent::KeyDown {
            scancode: 30,
            character: Some('a'),
        });

        let notifications = comp.drain_notifications();
        // Should have focus notification from create + key event.
        let key_event = notifications.iter().find(|n| {
            matches!(
                n,
                EventNotification::KeyEvent {
                    window_id,
                    scancode: 30,
                    pressed: true,
                    ..
                } if *window_id == id
            )
        });
        assert!(key_event.is_some());
    }

    /// Decode what `drain_input_frame` produced, as a client would.
    fn decode_drained(comp: &mut Compositor) -> Vec<guiremote::InputEvent> {
        let frame = comp
            .drain_input_frame()
            .expect("expected at least one pending event");
        let (events, used) =
            guiremote::decode_input_frame(&frame).expect("compositor emitted an undecodable frame");
        assert_eq!(used, frame.len(), "frame must decode in full");
        events
    }

    #[test]
    fn a_key_press_reaches_a_client_as_a_named_key_over_the_wire() {
        // The end-to-end path that did not exist: hardware scancode in one
        // side, a `guitk::event::Event` a widget tree can consume out the
        // other, having actually crossed the wire codec in between. Asserting
        // on `EventNotification` alone would leave the encode/decode pair
        // untested against a real compositor's output.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Focused".to_string(), 400, 300, 1);

        // 0x1E is `A` in scan code set 1.
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1E,
            character: Some('a'),
        });

        let key = decode_drained(&mut comp)
            .into_iter()
            .find(|e| matches!(e.event, ClientEvent::Key(_)))
            .expect("no key event survived the round trip");

        assert_eq!(key.window, id.0, "must be addressed to the focused window");
        assert_eq!(key.scancode, Some(0x1E), "the raw code rides along (§456)");
        let ClientEvent::Key(k) = key.event else {
            unreachable!("filtered to key events above")
        };
        assert_eq!(k.key, Key::A);
        assert!(k.pressed);
        assert_eq!(k.text, "a");
        assert_eq!(k.modifiers, Modifiers::NONE);
    }

    #[test]
    fn a_chord_reports_the_modifier_that_formed_it() {
        // The ordering trap: fold the modifier into the state *after* building
        // the notification and Ctrl+S arrives as a bare S, which every
        // keyboard shortcut in the system would then miss.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1D, // left ctrl
            character: None,
        });
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F, // S
            character: Some('s'),
        });

        let events = decode_drained(&mut comp);
        let chord = events
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::Key(k) if k.key == Key::S => Some(k),
                _ => None,
            })
            .expect("no S key event");
        assert!(chord.modifiers.ctrl, "Ctrl+S must arrive as a chord");

        // And the modifier's own event reports itself held, rather than being
        // one event behind.
        let ctrl = events
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::Key(k) if k.key == Key::LeftCtrl => Some(k),
                _ => None,
            })
            .expect("no Ctrl key event");
        assert!(ctrl.modifiers.ctrl);
    }

    #[test]
    fn arrow_keys_keep_their_extended_prefix_end_to_end() {
        // The keys a text editor uses most, and the ones that collide with the
        // keypad if the prefix is lost anywhere along the path.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);

        for (scancode, expected) in [
            (0xE04Bu32, Key::Left),
            (0xE04D, Key::Right),
            (0xE048, Key::Up),
            (0xE050, Key::Down),
        ] {
            comp.handle_input(InputEvent::KeyDown {
                scancode,
                character: None,
            });
            let got = decode_drained(&mut comp)
                .into_iter()
                .find_map(|e| match e.event {
                    ClientEvent::Key(k) => Some(k.key),
                    _ => None,
                })
                .expect("no key event");
            assert_eq!(got, expected, "scancode {scancode:#x}");
        }
    }

    #[test]
    fn a_key_release_survives_the_wire_as_a_release() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x39,
            character: Some(' '),
        });
        comp.handle_input(InputEvent::KeyUp { scancode: 0x39 });

        let states: Vec<bool> = decode_drained(&mut comp)
            .into_iter()
            .filter_map(|e| match e.event {
                ClientEvent::Key(k) if k.key == Key::Space => Some(k.pressed),
                _ => None,
            })
            .collect();
        assert_eq!(states, vec![true, false]);
    }

    /// A settings value that differs from the default in the layout alone.
    ///
    /// Built by struct update rather than by assigning into a `default()`,
    /// so that a field added to either config later is carried by the
    /// `..Default::default()` instead of being silently left unset here.
    fn settings_using(layout: &str) -> InputSettings {
        InputSettings {
            keyboard: inputsettings::KeyboardConfig {
                layout: layout.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// The first key event a client is handed, decoded off the wire.
    fn first_key(comp: &mut Compositor) -> ClientKeyEvent {
        decode_drained(comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Key(k) => Some(k),
                _ => None,
            })
            .expect("no key event reached the client")
    }

    #[test]
    fn a_keystroke_arrives_as_the_letter_the_chosen_layout_puts_there() {
        // The whole point of `keylayout`: the same physical key means a
        // different letter once the user has chosen a different layout. On
        // Dvorak the key engraved `S` types `o`, and a compositor that still
        // consulted the physical table alone would send `s` — which is the
        // state this replaced, where choosing a layout in Settings changed
        // the picture of the keyboard and nothing else.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("dvorak"));

        // 0x1F is the physical `S` key in scan code set 1; no character comes
        // from the source, exactly as the evdev backend delivers it.
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F,
            character: None,
        });

        let k = first_key(&mut comp);
        assert_eq!(k.key, Key::O, "Dvorak types `o` where the board says `S`");
        assert_eq!(k.text, "o", "and the text channel must agree");
    }

    #[test]
    fn a_character_the_source_supplies_beats_the_layout() {
        // The host backend fills `character` from the platform's own
        // translation (`WM_CHAR` on Windows), which has already applied
        // whatever layout the *host* is using. Re-translating it here would
        // apply two layouts to one keystroke.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("dvorak"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F,
            character: Some('s'),
        });

        assert_eq!(first_key(&mut comp).text, "s");
    }

    #[test]
    fn a_dead_key_composes_with_the_next_letter_end_to_end() {
        // The unit tests in `deadkey` prove the rules; this proves the wiring,
        // which is the half that can be right in isolation and still never
        // run. German `´` is the key right of `ß` (0x0D).
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x0D,
            character: None,
        });
        assert_eq!(
            first_key(&mut comp).text,
            "",
            "a dead key types nothing on its own"
        );

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x12, // E
            character: None,
        });
        assert_eq!(first_key(&mut comp).text, "é");
    }

    #[test]
    fn a_failed_composition_reaches_the_client_as_both_characters() {
        // `design-decisions.md` §550, end to end. This is the case that made
        // `KeyEvent::text` a `String`: one keystroke, two characters, and a
        // field that dropped either of them would be eating input.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x0D,
            character: None,
        });
        let _ = first_key(&mut comp);
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x2D, // X — there is no "x with acute"
            character: None,
        });
        assert_eq!(first_key(&mut comp).text, "´x");
    }

    #[test]
    fn moving_focus_disarms_a_pending_dead_key() {
        // An accent belongs to the window it was armed in. Carried across a
        // focus change it would put a letter the user never typed into a
        // document they had already left.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let first = comp.create_window("First".to_string(), 400, 300, 1);
        let second = comp.create_window("Second".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.focus_window(first);
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x0D,
            character: None,
        });
        let _ = first_key(&mut comp);

        comp.focus_window(second);
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x12, // E
            character: None,
        });
        assert_eq!(
            first_key(&mut comp).text,
            "e",
            "the accent followed focus into the next window"
        );
    }

    #[test]
    fn losing_the_keyboard_disarms_a_pending_dead_key() {
        // A VT switch or a device unplug. The vowel that would have completed
        // the accent is never coming; keeping it armed would attach it to the
        // first letter typed after the session comes back.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x0D,
            character: None,
        });
        let _ = first_key(&mut comp);

        comp.release_all_modifiers();
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x12, // E
            character: None,
        });
        assert_eq!(first_key(&mut comp).text, "e");
    }

    #[test]
    fn a_us_layout_types_a_grave_accent_as_a_grave_accent() {
        // The regression that would matter most: US QWERTY declares no dead
        // keys, and every shell prompt, every Markdown code span and every
        // `git log --format` string is a backtick. A machine that armed on the
        // character rather than on the layout's declaration would break all of
        // them.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("us-qwerty"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x29, // the key left of `1`
            character: None,
        });
        assert_eq!(first_key(&mut comp).text, "`");
    }

    #[test]
    fn alt_gr_types_a_character_rather_than_forming_an_alt_chord() {
        // On German there is no other `@` on the board, so AltGr+Q has to
        // produce one. It must *not* also arrive as Alt+Q: the menu bar
        // answers Alt chords first, so a German user typing an e-mail address
        // would open a menu instead.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0xE038, // right alt, i.e. AltGr
            character: None,
        });
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x10, // Q
            character: None,
        });

        let k = decode_drained(&mut comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Key(k) if k.key == Key::Q => Some(k),
                _ => None,
            })
            .expect("no Q key event");
        assert_eq!(k.text, "@", "AltGr+Q is the only `@` on a German board");
        assert!(
            !k.modifiers.alt,
            "AltGr spent itself on the character and must not also read as Alt"
        );
    }

    #[test]
    fn alt_gr_still_reads_as_alt_where_the_layout_has_nothing_on_that_level() {
        // The other half of the rule: clearing `alt` unconditionally would
        // cost every AltGr chord an application defines for itself on a
        // layout that puts no character on the third level.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("us-qwerty"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0xE038,
            character: None,
        });
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x10,
            character: None,
        });

        let k = decode_drained(&mut comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Key(k) if k.key == Key::Q => Some(k),
                _ => None,
            })
            .expect("no Q key event");
        assert!(
            k.modifiers.alt,
            "US QWERTY has no third level to spend it on"
        );
    }

    #[test]
    fn a_letter_the_key_enum_cannot_name_still_types_its_character() {
        // German puts `ü` where US QWERTY has `[`, and `guitk::Key` has no
        // name for `ü`. The character channel carries it; `key` falls back to
        // the physical identity so a shortcut bound to that key still fires.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("de-qwertz"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1A, // `[` on US QWERTY
            character: None,
        });

        let k = first_key(&mut comp);
        assert_eq!(k.text, "ü");
        assert_eq!(
            k.key,
            keymap::key_for_scancode(0x1A),
            "an unnameable character falls back to the physical key"
        );
    }

    #[test]
    fn a_release_carries_no_text() {
        // A client that inserted text on every key event would double every
        // letter if the release carried one too.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("dvorak"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F,
            character: None,
        });
        comp.handle_input(InputEvent::KeyUp { scancode: 0x1F });

        let texts: Vec<String> = decode_drained(&mut comp)
            .into_iter()
            .filter_map(|e| match e.event {
                ClientEvent::Key(k) if k.key == Key::O => Some(k.text),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["o".to_string(), String::new()]);
    }

    #[test]
    fn a_layout_this_build_does_not_know_leaves_the_keyboard_working() {
        // A settings file written by a later build — or edited by hand — must
        // not be able to leave the user with a keyboard that types nothing.
        // The name is preserved in the file (see `inputsettings`); what is
        // refused is *acting* on it.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("dvorak"));
        comp.set_input_settings(settings_using("klingon-plqaD"));

        assert_eq!(
            comp.keyboard_layout().id,
            "dvorak",
            "an unknown id must leave the layout already in force alone"
        );
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F,
            character: None,
        });
        assert_eq!(first_key(&mut comp).text, "o");
    }

    #[test]
    fn shift_and_caps_lock_reach_the_client_as_the_upper_face() {
        // The level machinery is `keylayout`'s, but the wiring that hands it
        // the modifier state is the compositor's, and it is the wiring that
        // was missing.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.set_input_settings(settings_using("us-qwerty"));

        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x2A, // left shift
            character: None,
        });
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1E, // A
            character: None,
        });

        let k = decode_drained(&mut comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Key(k) if k.key == Key::A => Some(k),
                _ => None,
            })
            .expect("no A key event");
        assert_eq!(k.text, "A");
    }

    #[test]
    fn a_mouse_click_arrives_in_window_local_coordinates() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Target".to_string(), 400, 300, 1);
        let (wx, wy) = {
            let w = comp.window_ref(id).expect("window just created");
            (w.x, w.y)
        };

        // Twenty pixels into the client area from the window's own origin.
        comp.handle_input(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            x: wx + 20,
            y: wy + 30,
        });

        let click = decode_drained(&mut comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Mouse(m) if matches!(m.kind, ClientMouseKind::Press(_)) => Some(m),
                _ => None,
            })
            .expect("no press event");
        // A client knows nothing about where it sits on screen, so an absolute
        // coordinate here would be unusable to it.
        assert!(
            (click.x - 20.0).abs() < 1.0 && (click.y - 30.0).abs() < 1.0,
            "expected roughly (20, 30) window-local, got ({}, {})",
            click.x,
            click.y
        );
    }

    #[test]
    fn a_quiet_tick_sends_no_frame_at_all() {
        // Input is bursty; most ticks have none. An empty frame every 16ms is
        // pure overhead on an idle desktop.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Idle".to_string(), 400, 300, 1);
        let _ = comp.drain_input_frame(); // clear the creation/focus events
        assert!(comp.drain_input_frame().is_none());
    }

    #[test]
    fn a_modifier_held_when_the_keyboard_is_lost_does_not_stay_stuck() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        comp.create_window("Focused".to_string(), 400, 300, 1);
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1D,
            character: None,
        });
        assert!(comp.modifiers().ctrl);

        comp.release_all_modifiers();
        assert_eq!(
            comp.modifiers(),
            Modifiers::NONE,
            "a session switch must not leave Ctrl down forever"
        );

        let _ = comp.drain_input_frame();
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x1F,
            character: Some('s'),
        });
        let k = decode_drained(&mut comp)
            .into_iter()
            .find_map(|e| match e.event {
                ClientEvent::Key(k) => Some(k),
                _ => None,
            })
            .expect("no key event");
        assert!(!k.modifiers.ctrl, "later keys must not arrive as chords");
    }

    #[test]
    fn a_modifier_pressed_with_no_focused_window_is_still_tracked() {
        // No window means no notification, but the key is physically down all
        // the same. Skipping the state update would make every event after it
        // wrong.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        assert!(comp.focused_window.is_none());
        comp.handle_input(InputEvent::KeyDown {
            scancode: 0x2A, // left shift
            character: None,
        });
        assert!(comp.modifiers().shift);
    }

    #[test]
    fn test_display_resize() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        assert!(comp.resize_display(1920, 1080).is_ok());
        assert_eq!(comp.backend.size(), (1920, 1080));
        assert!(comp.full_recomposite);
    }

    #[test]
    fn test_color_to_argb() {
        let color = Color::rgba(255, 128, 64, 200);
        let argb = color_to_argb(&color);
        assert_eq!((argb >> 24) & 0xFF, 200);
        assert_eq!((argb >> 16) & 0xFF, 255);
        assert_eq!((argb >> 8) & 0xFF, 128);
        assert_eq!(argb & 0xFF, 64);
    }

    /// A window on the ordinary terms — decorated, resizable, opaque — at the
    /// given place and size.
    fn plain_window(x: i32, y: i32, width: u32, height: u32) -> Window {
        Window::from_spec(&WindowSpec::new("Test", width, height), x, y, 1)
    }

    #[test]
    fn test_window_rects() {
        let win = plain_window(100, 100, 400, 300);
        let client = win.client_rect();
        assert_eq!(client, Rect::new(100, 100, 400, 300));

        let title_bar = win.title_bar_rect().expect("a decorated window has a bar");
        assert_eq!(title_bar.y, 100 - TITLE_BAR_HEIGHT as i32);
        assert_eq!(title_bar.height, TITLE_BAR_HEIGHT);

        let close = win.close_button_rect().expect("a decorated window closes");
        // Close button should be within the title bar.
        assert!(title_bar.contains(close.x, close.y));
    }

    #[test]
    fn an_undecorated_window_is_all_client_area() {
        let mut spec = WindowSpec::new("Menu", 200, 120);
        spec.decorations = false;
        let win = Window::from_spec(&spec, 40, 50, 1);

        // No frame anywhere: not in the insets, not in the hit-test rects, and
        // not in the bounds used for damage. A menu that reserved 30px for a
        // title bar it never draws would leave a strip of stale desktop.
        assert_eq!(win.frame_insets(), (0, 0, 0));
        assert_eq!(win.shadow_extent(), 0);
        assert_eq!(win.title_bar_rect(), None);
        assert_eq!(win.close_button_rect(), None);
        assert_eq!(win.minimize_button_rect(), None);
        assert_eq!(win.title_bar_layout(), None);
        assert_eq!(win.outer_rect(), win.client_rect());
    }

    #[test]
    fn fullscreen_suppresses_decorations_without_forgetting_them() {
        let mut win = plain_window(0, 0, 800, 600);
        win.fullscreen = true;
        assert!(!win.is_framed());
        assert_eq!(win.title_bar_rect(), None);

        // The client asked for decorations and never withdrew the request, so
        // leaving fullscreen must bring the frame back.
        win.fullscreen = false;
        assert!(win.is_framed());
        assert!(win.title_bar_rect().is_some());
    }

    #[test]
    fn a_non_resizable_window_has_no_maximize_button_and_minimize_takes_its_place() {
        let mut spec = WindowSpec::new("Dialog", 300, 200);
        spec.resizable = false;
        let win = Window::from_spec(&spec, 10, 10, 1);

        assert_eq!(win.maximize_button_rect(), None);
        let close = win.close_button_rect().expect("close is always there");
        let minimize = win
            .minimize_button_rect()
            .expect("minimize is always there");
        // Adjacent to close, not one slot further left: the gap a missing
        // button leaves would otherwise be a dead patch of title bar.
        assert_eq!(
            minimize.x,
            close.x - (TITLE_BUTTON_SIZE + TITLE_BUTTON_SPACING) as i32
        );

        let layout = win.title_bar_layout().expect("still decorated");
        assert_eq!(layout.button_count(), 2);
    }

    #[test]
    fn a_border_drag_is_refused_on_a_window_that_cannot_be_resized() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");

        let mut fixed = WindowSpec::new("Fixed", 200, 150);
        fixed.resizable = false;
        fixed.position = Some((100, 100));
        let fixed_id = comp.create_window_from_spec(&fixed, 1);

        let mut free = WindowSpec::new("Free", 200, 150);
        free.position = Some((400, 100));
        let free_id = comp.create_window_from_spec(&free, 1);

        // A point one pixel outside the left client edge is on the border.
        let on_left_border = |win: &Window| (win.x - 1, win.y + 10);

        let win = comp.window_ref(free_id).expect("free window").clone();
        let (x, y) = on_left_border(&win);
        assert!(comp.detect_border_drag(&win, x, y).is_some());

        let win = comp.window_ref(fixed_id).expect("fixed window").clone();
        let (x, y) = on_left_border(&win);
        assert_eq!(comp.detect_border_drag(&win, x, y), None);
    }

    #[test]
    fn nothing_a_window_draws_falls_outside_its_damage_extent() {
        // `window_drawn_extent` is what damage tracking repaints. A decoration
        // that paints outside it leaves a smear nothing ever cleans up, and the
        // old version's only guarantee was a comment saying it "mirrors the
        // geometry in render_shadow" — which nothing checked, and which had
        // stopped being true. Render a framed window over a known background
        // and assert every changed pixel is inside the extent.
        let mut comp = Compositor::new(400, 300, 60).expect("compositor");
        let mut spec = WindowSpec::new("Framed", 120, 90);
        spec.position = Some((140, 120));
        let id = comp.create_window_from_spec(&spec, 1);

        let bg = comp.theme.desktop_background;
        comp.backend.clear(bg);
        comp.render_window(id);

        let extent = comp
            .window_ref(id)
            .map(Compositor::window_drawn_extent)
            .expect("window");
        for y in 0..300u32 {
            for x in 0..400u32 {
                if working_pixel(&comp.backend, x, y) == Some(bg) {
                    continue;
                }
                assert!(
                    extent.contains(x as i32, y as i32),
                    "painted ({x}, {y}), outside the damage extent {extent:?}"
                );
            }
        }
    }

    #[test]
    fn the_outer_rect_is_the_frame_rect_plus_the_shadow() {
        // outer_rect is now defined as frame_rect().inflate(shadow_extent()).
        // These are the numbers that definition has to keep producing, written
        // out from the constants so a change to either helper has to be
        // deliberate rather than merely compile.
        let win = plain_window(100, 100, 200, 150);
        assert_eq!(win.frame_rect(), Rect::new(99, 70, 202, 181));
        assert_eq!(win.outer_rect(), Rect::new(91, 62, 218, 197));
        assert_eq!(win.frame_rect().inflate(SHADOW_SIZE), win.outer_rect());
        // The title bar occupies the top inset of the frame box exactly.
        let bar = win.title_bar_rect().expect("framed");
        assert_eq!(bar, Rect::new(99, 70, 202, TITLE_BAR_HEIGHT));
        assert_eq!(bar.x, win.frame_rect().x);
        assert_eq!(bar.width, win.frame_rect().width);
    }

    #[test]
    fn maximising_makes_the_frame_flush_with_the_display_not_the_client_area() {
        // `client_geometry_for_frame` is the inverse of `frame_rect`, and that
        // is the property maximise depends on: what the user sees touching the
        // screen edge is the decorated box, not the client area inside it.
        // Round-tripping both ways is what stops the two drifting apart.
        for &(framed, w, h) in &[(true, 200u32, 150u32), (false, 200, 150), (true, 640, 480)] {
            let mut win = plain_window(37, 91, w, h);
            win.decorations = framed;
            let area = Rect::new(-11, 23, 1920, 1080);
            let (x, y, fw, fh) = win.client_geometry_for_frame(area);

            let mut placed = win.clone();
            placed.x = x;
            placed.y = y;
            placed.width = fw;
            placed.height = fh;
            assert_eq!(
                placed.frame_rect(),
                area,
                "a window fitted to {area:?} must have exactly that frame box (framed={framed})"
            );
        }
    }

    #[test]
    fn maximise_fills_the_display_with_the_frame() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = comp.create_window("Max".to_string(), 200, 150, 1);
        comp.maximize_window(id).expect("maximize");
        let bounds = comp.display_manager.virtual_bounds();
        let win = comp.window_ref(id).expect("window");
        assert_eq!(
            win.frame_rect(),
            bounds,
            "the decorated box, not the client area, fills the display"
        );
    }

    #[test]
    fn a_shadow_with_negative_spread_draws_nothing_rather_than_everything() {
        // A spread more negative than the blur shrinks the shadow past nothing.
        // Cast rather than clamped, the negative extent reappeared as ~4e9
        // pixels wide, and the "shadow" filled everything from its own origin
        // to the far corner of the clip.
        const BG: u32 = 0xFF00_0000;
        let mut fb = Framebuffer::new(64, 64).expect("framebuffer");
        fb.clear(BG);
        let mut engine = RenderEngine::new();
        engine.execute(
            &mut fb,
            &[RenderCommand::BoxShadow {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
                offset_x: 0.0,
                offset_y: 0.0,
                blur: 1.0,
                spread: -40.0,
                color: Color::rgba(255, 255, 255, 255),
                corner_radii: CornerRadii::ZERO,
            }],
            0,
            0,
            64,
            64,
            1.0,
        );
        for y in 0..64u32 {
            for x in 0..64u32 {
                assert_eq!(
                    fb.get_pixel(x, y),
                    Some(BG),
                    "a shadow smaller than nothing must draw nothing, but ({x},{y}) changed"
                );
            }
        }
    }

    /// The line rasteriser exactly as it stood before it was clipped and
    /// widened to `i64` — the reference the new one must agree with.
    ///
    /// Only called with endpoints small enough that the original could not
    /// overflow, which is the whole domain on which it had a defined answer.
    ///
    /// Deliberately a character-for-character transcription — do not tidy the
    /// arithmetic here. Its value as a reference is that it *is* the old code;
    /// an improved reference would prove nothing about what shipped.
    fn unclipped_bresenham(x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
        let dx = (x2 - x1).abs();
        let dy = -(y2 - y1).abs();
        let sx: i32 = if x1 < x2 { 1 } else { -1 };
        let sy: i32 = if y1 < y2 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut cx, mut cy) = (x1, y1);
        let mut out = Vec::new();
        loop {
            out.push((cx, cy));
            if cx == x2 && cy == y2 {
                return out;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
    }

    #[test]
    fn line_matches_the_unclipped_bresenham_walk() {
        // Clipping a line to the screen is only allowed to drop pixels that
        // were invisible anyway. Every endpoint pair in a grid that fits
        // entirely on the framebuffer must therefore light exactly the pixels
        // the original loop did — every octant, both diagonals, both axes,
        // and the degenerate single-point case.
        let engine = RenderEngine::new();
        for x1 in 0..7i32 {
            for y1 in 0..7i32 {
                for x2 in 0..7i32 {
                    for y2 in 0..7i32 {
                        let mut fb = Framebuffer::new(7, 7).expect("framebuffer");
                        fb.clear(0xFF_00_00_00);
                        engine.draw_line(&mut fb, x1, y1, x2, y2, 0xFF_FF_FF_FF, 1.0);

                        let mut want = vec![vec![false; 7]; 7];
                        for (px, py) in unclipped_bresenham(x1, y1, x2, y2) {
                            if let Some(row) = want.get_mut(py as usize) {
                                if let Some(cell) = row.get_mut(px as usize) {
                                    *cell = true;
                                }
                            }
                        }
                        for py in 0..7u32 {
                            for px in 0..7u32 {
                                let lit = fb.get_pixel(px, py) == Some(0xFF_FF_FF_FF);
                                let expect = want[py as usize][px as usize];
                                assert_eq!(
                                    lit, expect,
                                    "({x1},{y1})->({x2},{y2}) at pixel ({px},{py})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_line_across_the_whole_coordinate_space_neither_hangs_nor_overflows() {
        // A client can send any f32 endpoints it likes; these are what they
        // saturate to. The old loop needed 2^32 iterations to reach the screen
        // and panicked in `.abs()` before it got there.
        let engine = RenderEngine::new();
        let mut fb = Framebuffer::new(64, 64).expect("framebuffer");
        let far = [
            (i32::MIN, i32::MIN, i32::MAX, i32::MAX),
            (i32::MIN, 5, i32::MAX, 6),
            (i32::MAX, i32::MIN, i32::MIN, i32::MAX),
            (i32::MIN, i32::MAX, 32, 32),
            // Wholly off-screen: must draw nothing rather than walk there.
            (-9000, -9000, -8000, -8000),
        ];
        for (x1, y1, x2, y2) in far {
            fb.clear(0xFF_00_00_00);
            engine.draw_line(&mut fb, x1, y1, x2, y2, 0xFF_FF_FF_FF, 1.0);
        }
        // The last case crosses no framebuffer pixel at all.
        assert!(
            (0..64).all(|y| (0..64).all(|x| fb.get_pixel(x, y) == Some(0xFF_00_00_00))),
            "an off-screen line painted something"
        );
    }

    /// Start a border drag on `id` at `(x, y)` and move the pointer by
    /// `(dx, dy)`, as the user would.
    fn drag_border(comp: &mut Compositor, id: WindowId, x: i32, y: i32, dx: i32, dy: i32) {
        comp.handle_mouse_button(MouseButton::Left, true, x, y);
        let Some(drag) = comp.drag.as_ref() else {
            panic!("no drag started at ({x}, {y})");
        };
        assert_eq!(drag.window_id, id, "the drag grabbed the wrong window");
        comp.handle_mouse_move(x.saturating_add(dx), y.saturating_add(dy));
    }

    #[test]
    fn every_resize_edge_moves_the_edge_the_user_grabbed() {
        // One case per resize mode, checking the pair the old nine-arm match
        // had to get right nine separate times: which extent changes, and
        // whether the origin follows.
        //
        // The three top grabs sit at `-35`, not `-1`: the title bar occupies
        // `y ∈ [win.y - TITLE_BAR_HEIGHT, win.y)`, and a point in it starts a
        // *move*, not a resize. The top resize band is the strip above it,
        // inside the shadow that `outer_rect` covers.
        let cases = [
            // (grab point relative to the window, dx, dy, expected rect)
            ("right", (200, 75), (50, 0), (100, 100, 250, 150)),
            ("left", (-1, 75), (-50, 0), (50, 100, 250, 150)),
            ("bottom", (100, 150), (0, 40), (100, 100, 200, 190)),
            ("top", (100, -35), (0, -40), (100, 60, 200, 190)),
            ("top-left", (-5, -35), (-50, -40), (50, 60, 250, 190)),
            ("top-right", (195, -35), (50, -40), (100, 60, 250, 190)),
            ("bottom-left", (-1, 150), (-50, 40), (50, 100, 250, 190)),
            ("bottom-right", (200, 150), (50, 40), (100, 100, 250, 190)),
        ];
        for (name, (gx, gy), (dx, dy), want) in cases {
            let mut comp = Compositor::new(800, 600, 60).expect("compositor");
            let mut spec = WindowSpec::new("Resizable", 200, 150);
            spec.position = Some((100, 100));
            let id = comp.create_window_from_spec(&spec, 1);
            drag_border(&mut comp, id, 100 + gx, 100 + gy, dx, dy);
            let win = comp.window_ref(id).expect("window");
            assert_eq!(
                (win.x, win.y, win.width, win.height),
                want,
                "dragging the {name} edge"
            );
        }
    }

    // ---- double-click the title bar to maximize ---------------------------
    //
    // The gesture every desktop has. It used to live in the desktop shell,
    // which drew a second copy of every title bar and hit-tested its own copy;
    // when that duplicate was deleted the gesture came here, beside the hit
    // test that already turns a press on this same strip into a move, a close
    // or a resize. Timing lives here too — the shell no longer has to be told
    // which press was a "double" one.

    /// A full click near the left end of `id`'s title bar, away from the
    /// buttons at its right-hand end. Asked of the window each time because a
    /// maximized window's title bar is somewhere else.
    fn click_title_bar(comp: &mut Compositor, id: WindowId) {
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("a decorated window has a title bar");
        let x = bar.x + 10;
        let y = bar.y + bar.height as i32 / 2;
        comp.handle_mouse_button(MouseButton::Left, true, x, y);
        comp.handle_mouse_button(MouseButton::Left, false, x, y);
    }

    fn maximized(comp: &Compositor, id: WindowId) -> bool {
        comp.window_ref(id).expect("window").maximized
    }

    /// A compositor with one resizable window at (100, 100), and a
    /// double-click interval long enough that no scheduling delay between two
    /// synchronous calls can exceed it.
    fn with_one_window() -> (Compositor, WindowId) {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        comp.set_double_click_ms(2000);
        let mut spec = WindowSpec::new("Resizable", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        (comp, id)
    }

    #[test]
    fn a_double_click_on_the_title_bar_maximizes_and_a_second_one_restores() {
        let (mut comp, id) = with_one_window();

        click_title_bar(&mut comp, id);
        assert!(!maximized(&comp, id), "one click is not the gesture");
        click_title_bar(&mut comp, id);
        assert!(maximized(&comp, id), "two quick clicks must maximize");

        click_title_bar(&mut comp, id);
        assert!(
            maximized(&comp, id),
            "the third click pairs with nothing — the second consumed the pair"
        );
        click_title_bar(&mut comp, id);
        assert!(!maximized(&comp, id), "the gesture toggles");
    }

    /// The first click of a pair still begins a move. A title bar that only
    /// responded on the second click could not be dragged at all.
    #[test]
    fn the_first_click_of_the_pair_still_starts_a_move() {
        let (mut comp, id) = with_one_window();
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("title bar");
        comp.handle_mouse_button(MouseButton::Left, true, bar.x + 10, bar.y + 5);
        let drag = comp.drag.as_ref().expect("a title press starts a move");
        assert_eq!(drag.window_id, id);
        assert!(matches!(drag.mode, DragMode::MoveWindow));
    }

    /// Two clicks minutes apart are two clicks. Nothing else in the compositor
    /// measures elapsed time, so the interval has to actually be consulted.
    #[test]
    fn two_title_clicks_far_enough_apart_are_two_separate_clicks() {
        let (mut comp, id) = with_one_window();
        // The shortest interval the setter allows; the sleep is comfortably
        // past it, and can only ever overshoot, never undershoot.
        comp.set_double_click_ms(100);

        click_title_bar(&mut comp, id);
        std::thread::sleep(Duration::from_millis(160));
        click_title_bar(&mut comp, id);
        assert!(
            !maximized(&comp, id),
            "clicks outside the interval must not pair"
        );
    }

    /// Two windows, one click each, as fast as the machine can manage. Pairing
    /// them would maximize a window the user clicked exactly once — a window
    /// moving on its own.
    #[test]
    fn a_quick_click_on_each_of_two_title_bars_maximizes_neither() {
        let (mut comp, first) = with_one_window();
        let mut spec = WindowSpec::new("Second", 200, 150);
        spec.position = Some((400, 100));
        let second = comp.create_window_from_spec(&spec, 2);

        click_title_bar(&mut comp, first);
        click_title_bar(&mut comp, second);
        assert!(!maximized(&comp, first));
        assert!(!maximized(&comp, second));
    }

    /// The user went somewhere else and came back. However fast that was, it
    /// was not a double-click.
    #[test]
    fn a_press_somewhere_else_between_two_title_clicks_breaks_the_pair() {
        let (mut comp, id) = with_one_window();

        click_title_bar(&mut comp, id);
        // The bare desktop, well clear of the window and its shadow.
        comp.handle_mouse_button(MouseButton::Left, true, 700, 500);
        comp.handle_mouse_button(MouseButton::Left, false, 700, 500);
        click_title_bar(&mut comp, id);
        assert!(!maximized(&comp, id));
    }

    /// A value from the mouse settings panel arrives as a `u32` of milliseconds
    /// and is not this crate's to trust. Zero in particular would make the
    /// gesture impossible to perform rather than merely hard.
    #[test]
    fn the_double_click_interval_is_clamped_to_a_performable_range() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        comp.set_double_click_ms(0);
        assert_eq!(
            comp.double_click_interval,
            Duration::from_millis(u64::from(MIN_DOUBLE_CLICK_MS))
        );
        comp.set_double_click_ms(u32::MAX);
        assert_eq!(
            comp.double_click_interval,
            Duration::from_millis(u64::from(MAX_DOUBLE_CLICK_MS))
        );
        // And a value inside the range is passed through untouched — a clamp
        // that returned a bound for everything would satisfy the two above.
        comp.set_double_click_ms(250);
        assert_eq!(comp.double_click_interval, Duration::from_millis(250));
    }

    // ======================================================================
    // Acting on a window on a shell's behalf
    // ======================================================================

    /// The whole point of the operation: a taskbar button on a minimized
    /// window gives it back *and* focuses it.
    ///
    /// This is the ordering invariant `activate_window` exists to hold.
    /// `focus_window` refuses a minimized window on purpose — a window nobody
    /// can see must not hold the keyboard — so un-minimizing has to happen
    /// first. Swap the two halves of `activate_window` and this fails on the
    /// focus assertion while the visibility one still passes, which is exactly
    /// the half-working button a user would report as "it comes back but I
    /// have to click it again".
    #[test]
    fn activating_a_minimized_window_unminimizes_it_and_gives_it_focus() {
        let (mut comp, id) = with_one_window();
        comp.minimize_window(id).expect("minimize");
        assert_ne!(comp.focused_window, Some(id), "minimizing dropped focus");

        comp.activate_window(id).expect("activate");

        let win = comp.window_ref(id).expect("window");
        assert!(!win.minimized, "still minimized");
        assert!(win.visible, "un-minimized but still hidden");
        assert_eq!(
            comp.focused_window,
            Some(id),
            "un-minimized but not focused"
        );
    }

    /// A taskbar button must give a window back exactly as the user left it.
    ///
    /// `restore_window` un-maximizes as well as un-minimizes, which is right
    /// for the client's own `Restore` request and wrong here: a window the
    /// user maximized and then minimized would come back at its old small
    /// size, having silently lost a state it never asked to leave. That is why
    /// `activate_window` is not a call to `restore_window`.
    #[test]
    fn activating_a_window_minimized_while_maximized_leaves_it_maximized() {
        let (mut comp, id) = with_one_window();
        comp.maximize_window(id).expect("maximize");
        comp.minimize_window(id).expect("minimize");

        comp.activate_window(id).expect("activate");
        assert!(
            maximized(&comp, id),
            "activating un-maximized a window the user never un-maximized"
        );

        // And the contrast is real rather than asserted: `restore_window` on
        // the same starting state does drop the maximize.
        comp.minimize_window(id).expect("minimize again");
        comp.restore_window(id).expect("restore");
        assert!(!maximized(&comp, id), "restore is supposed to un-maximize");
    }

    /// Activation raises within the band and no further. A taskbar button that
    /// lifted an application window over the taskbar would put the button
    /// under the window it just summoned.
    #[test]
    fn activating_a_window_does_not_lift_it_over_the_shell() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut app = WindowSpec::new("App", 200, 150);
        app.position = Some((100, 100));
        let app_id = comp.create_window_from_spec(&app, 1);

        let mut bar = WindowSpec::new("Taskbar", 800, 40);
        bar.position = Some((0, 560));
        bar.layer = Layer::Overlay;
        let bar_id = comp.create_window_from_spec(&bar, 2);

        comp.activate_window(app_id).expect("activate");

        let app_at = comp.z_stack.iter().position(|&w| w == app_id);
        let bar_at = comp.z_stack.iter().position(|&w| w == bar_id);
        assert!(
            app_at < bar_at,
            "an activated application window climbed over the overlay band"
        );
    }

    /// Activating a window that has already closed is a race, not a fault: the
    /// click lands after the list the button was drawn from. It must report,
    /// not panic, and must leave focus where it was.
    #[test]
    fn activating_a_window_that_has_gone_is_an_error_and_changes_nothing() {
        let (mut comp, id) = with_one_window();
        let ghost = WindowId::from_raw(id.raw().wrapping_add(999));
        let before = comp.focused_window;

        assert!(matches!(
            comp.activate_window(ghost),
            Err(CompositorError::WindowNotFound(_))
        ));
        assert_eq!(comp.focused_window, before);
    }

    /// Closing from a shell *asks*. The window is still there afterwards, and
    /// its client has been told — which is what lets an editor with unsaved
    /// changes put up its dialog instead of losing the user's work.
    #[test]
    fn a_shell_close_asks_the_window_rather_than_destroying_it() {
        let (mut comp, id) = with_one_window();

        comp.request_close(id).expect("close");

        assert!(
            comp.window_ref(id).is_some(),
            "a shell close destroyed the window outright"
        );
        assert!(
            comp.pending_notifications.iter().any(
                |n| matches!(n, EventNotification::WindowClose { window_id } if *window_id == id)
            ),
            "the client was never told to close"
        );
    }

    /// And a close aimed at a window that has gone queues nothing. A
    /// notification addressed to a dead id is one no link will ever claim,
    /// which is a slow leak in the pending queue rather than a visible bug.
    #[test]
    fn a_shell_close_of_a_window_that_has_gone_queues_no_notification() {
        let (mut comp, id) = with_one_window();
        let ghost = WindowId::from_raw(id.raw().wrapping_add(999));
        let before = comp.pending_notifications.len();

        assert!(matches!(
            comp.request_close(ghost),
            Err(CompositorError::WindowNotFound(_))
        ));
        assert_eq!(comp.pending_notifications.len(), before);
    }

    /// Every action reaches the operation it names.
    ///
    /// Listed rather than sampled: the dispatch is a five-arm match and an arm
    /// wired to the wrong method is invisible to a test that only sends one
    /// action. Each assertion is chosen to distinguish that arm from the other
    /// four.
    #[test]
    fn every_shell_control_action_reaches_its_own_operation() {
        let (mut comp, id) = with_one_window();

        let send = |comp: &mut Compositor, action| {
            comp.handle_request(CompositorRequest::ShellControl {
                window_id: id,
                action,
            })
        };

        assert!(matches!(
            send(&mut comp, ShellControlAction::Maximize),
            CompositorResponse::Ok
        ));
        assert!(maximized(&comp, id), "Maximize");

        assert!(matches!(
            send(&mut comp, ShellControlAction::Restore),
            CompositorResponse::Ok
        ));
        assert!(!maximized(&comp, id), "Restore");

        assert!(matches!(
            send(&mut comp, ShellControlAction::Minimize),
            CompositorResponse::Ok
        ));
        assert!(comp.window_ref(id).expect("window").minimized, "Minimize");

        assert!(matches!(
            send(&mut comp, ShellControlAction::Activate),
            CompositorResponse::Ok
        ));
        assert!(!comp.window_ref(id).expect("window").minimized, "Activate");

        assert!(matches!(
            send(&mut comp, ShellControlAction::SnapLeft),
            CompositorResponse::Ok
        ));
        assert_eq!(
            comp.window_ref(id).expect("window").snapped,
            Some(SnapTarget::Half(SnapEdge::Left)),
            "SnapLeft"
        );

        assert!(matches!(
            send(&mut comp, ShellControlAction::SnapRight),
            CompositorResponse::Ok
        ));
        assert_eq!(
            comp.window_ref(id).expect("window").snapped,
            Some(SnapTarget::Half(SnapEdge::Right)),
            "SnapRight"
        );
        assert!(matches!(
            send(&mut comp, ShellControlAction::Restore),
            CompositorResponse::Ok
        ));

        let before = comp.pending_notifications.len();
        assert!(matches!(
            send(&mut comp, ShellControlAction::Close),
            CompositorResponse::Ok
        ));
        assert!(comp.window_ref(id).is_some(), "Close must not destroy");
        assert!(comp.pending_notifications.len() > before, "Close");
    }

    /// The two halves must tile: together they cover the display exactly, with
    /// no gap and no overlap.
    ///
    /// Asserted on the *frame* rectangles rather than the client ones, because
    /// the client areas are inset by the decorations and legitimately do not
    /// touch. The seam this catches is the one an odd display width produces if
    /// each side is given `width / 2` independently — a one-pixel column
    /// belonging to neither half, straight down the middle of the screen, for
    /// the whole life of the session.
    #[test]
    fn the_two_snapped_halves_tile_the_display_with_no_seam() {
        // Odd width on purpose: an even one cannot show the bug.
        let mut comp = Compositor::new(801, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Left", 200, 150);
        spec.position = Some((10, 10));
        spec.decorations = false;
        let left = comp.create_window_from_spec(&spec, 1);
        let mut spec = WindowSpec::new("Right", 200, 150);
        spec.position = Some((20, 20));
        spec.decorations = false;
        let right = comp.create_window_from_spec(&spec, 1);

        comp.snap_window(left, SnapEdge::Left).expect("snap left");
        comp.snap_window(right, SnapEdge::Right)
            .expect("snap right");

        let l = comp.window_ref(left).expect("left");
        let r = comp.window_ref(right).expect("right");
        let bounds = comp.display_manager.virtual_bounds();

        assert_eq!(l.x, bounds.x, "the left half starts at the left edge");
        assert_eq!(
            l.x.saturating_add(i32::try_from(l.width).expect("small")),
            r.x,
            "the halves must meet exactly — a gap here is a visible seam"
        );
        assert_eq!(
            r.x.saturating_add(i32::try_from(r.width).expect("small")),
            bounds
                .x
                .saturating_add(i32::try_from(bounds.width).expect("small")),
            "the right half must reach the right edge"
        );
        assert_eq!(
            l.width.saturating_add(r.width),
            bounds.width,
            "together the halves are the whole display"
        );
    }

    /// Snapping remembers where the window was, and restoring puts it back.
    ///
    /// The interesting half is that this works at all: `restore_window` used to
    /// test `maximized` alone, so a snapped window — which is not maximized —
    /// was left exactly where it was and Super+Down appeared to do nothing.
    #[test]
    fn restoring_a_snapped_window_returns_it_to_where_it_was() {
        let (mut comp, id) = with_one_window();
        let before = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        comp.snap_window(id, SnapEdge::Left).expect("snap");
        assert_ne!(
            comp.window_ref(id).expect("window").width,
            before.width,
            "the snap did not resize anything, so the restore proves nothing"
        );

        comp.restore_window(id).expect("restore");
        let after = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };
        assert_eq!(after, before);
        assert_eq!(comp.window_ref(id).expect("window").snapped, None);
    }

    /// Maximize, then snap, then restore returns to the window's *own*
    /// geometry — not to the full-screen rectangle it had in between.
    ///
    /// Each of the three transitions is a chance to overwrite `restore_rect`
    /// with geometry the window only has because of the previous one, and the
    /// user-visible result is a window that can never be got back to its
    /// original size again.
    #[test]
    fn tiling_a_window_repeatedly_still_restores_to_where_it_started() {
        let (mut comp, id) = with_one_window();
        let before = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        comp.maximize_window(id).expect("maximize");
        comp.snap_window(id, SnapEdge::Left).expect("snap left");
        comp.snap_window(id, SnapEdge::Right).expect("snap right");
        comp.maximize_window(id).expect("maximize again");
        comp.restore_window(id).expect("restore");

        let w = comp.window_ref(id).expect("window");
        assert_eq!(Rect::new(w.x, w.y, w.width, w.height), before);
    }

    /// Snapping and maximizing are alternatives, so entering one leaves the
    /// other. A window recorded as both would leave `restore_window` choosing
    /// between two answers.
    /// Each direction has to *enter from the other state* to prove anything.
    /// Snapping a window that was never maximized cannot show that snapping
    /// clears `maximized`, because there was nothing to clear — the first
    /// version of this test made exactly that mistake and passed with the
    /// assignment deleted.
    #[test]
    fn a_window_is_never_snapped_and_maximized_at_once() {
        let (mut comp, id) = with_one_window();

        comp.maximize_window(id).expect("maximize");
        assert!(comp.window_ref(id).expect("window").maximized);
        comp.snap_window(id, SnapEdge::Left).expect("snap");
        assert!(
            !comp.window_ref(id).expect("window").maximized,
            "snapping a maximized window left it recorded as maximized too"
        );

        comp.maximize_window(id).expect("maximize again");
        assert_eq!(
            comp.window_ref(id).expect("window").snapped,
            None,
            "maximizing a snapped window left it recorded as snapped too"
        );
    }

    /// A window that declared itself non-resizable is not snapped, for the same
    /// reason it is not maximized: tiling it is a resize it said it cannot take.
    #[test]
    fn a_non_resizable_window_refuses_to_be_snapped() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Fixed", 200, 150);
        spec.position = Some((100, 100));
        spec.resizable = false;
        let id = comp.create_window_from_spec(&spec, 1);
        let before = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        assert!(matches!(
            comp.snap_window(id, SnapEdge::Left),
            Err(CompositorError::NotResizable(_))
        ));
        let w = comp.window_ref(id).expect("window");
        assert_eq!(Rect::new(w.x, w.y, w.width, w.height), before);
        assert_eq!(w.snapped, None);
    }

    /// The client is told its new size. A snap that resized the window in the
    /// compositor's records but never notified it would leave the program
    /// drawing at its old size inside a differently-sized frame.
    #[test]
    fn a_snapped_client_is_told_its_new_size() {
        let (mut comp, id) = with_one_window();
        comp.pending_notifications.clear();

        comp.snap_window(id, SnapEdge::Left).expect("snap");

        let width = comp.window_ref(id).expect("window").width;
        let height = comp.window_ref(id).expect("window").height;
        assert!(
            comp.pending_notifications.iter().any(|n| matches!(
                n,
                EventNotification::WindowResized { window_id, width: w, height: h }
                    if *window_id == id && *w == width && *h == height
            )),
            "the client was never told the snap resized it"
        );
    }

    // -----------------------------------------------------------------------
    // Zone snapping
    // -----------------------------------------------------------------------

    /// An undecorated window, so that its client rectangle *is* its frame and a
    /// placement assertion can be made against the zone rectangle directly
    /// rather than through the decoration insets.
    fn with_one_bare_window(width: u32, height: u32) -> (Compositor, WindowId) {
        let mut comp = Compositor::new(width, height, 60).expect("compositor");
        let mut spec = WindowSpec::new("Bare", 200, 150);
        spec.position = Some((100, 100));
        spec.decorations = false;
        let id = comp.create_window_from_spec(&spec, 1);
        (comp, id)
    }

    /// The window lands in the rectangle the shell's picker drew for that zone.
    ///
    /// This is the one assertion the whole arrangement exists for. The user
    /// aims at a rectangle painted on screen by the shell; the compositor,
    /// which never saw that painting, has to place the window in the *same*
    /// rectangle. Both sides get there through `guiremote::zones`, and this
    /// test computes the expected rectangle the way a shell would — from the
    /// display bounds, through the shared code — rather than restating the
    /// arithmetic, which would only prove the restatement matches itself.
    #[test]
    fn a_zone_snapped_window_lands_in_the_rectangle_the_picker_drew() {
        let (mut comp, id) = with_one_bare_window(801, 601);
        let bounds = comp.display_manager.virtual_bounds();

        for slot in SnapSlot::all() {
            let drawn = zone_rect(slot.rect(work_area_of(bounds)).expect("zone"));

            comp.snap_window_to_zone(id, slot).expect("snap to zone");

            let w = comp.window_ref(id).expect("window");
            assert_eq!(
                Rect::new(w.x, w.y, w.width, w.height),
                drawn,
                "zone {} of {:?} was drawn in one place and filled in another",
                slot.zone(),
                slot.preset()
            );
        }

        // A guard on the loop above: if `all` ever yielded nothing, every
        // assertion in it would vacuously pass.
        assert_eq!(SnapSlot::all().count(), usize::from(SnapSlot::COUNT));
    }

    /// What is stored is the slot, not the rectangle it currently resolves to.
    ///
    /// The same rule [`SnapEdge`] documents, for the same reason: a stored
    /// rectangle is silently wrong the moment the display changes size, and
    /// there is no later moment at which it would be noticed.
    #[test]
    fn a_zone_snapped_window_records_the_slot_and_not_its_geometry() {
        let (mut comp, id) = with_one_window();
        let slot = SnapSlot::new(SnapLayoutPreset::SixGrid, 4).expect("slot");

        comp.snap_window_to_zone(id, slot).expect("snap to zone");

        assert_eq!(
            comp.window_ref(id).expect("window").snapped,
            Some(SnapTarget::Zone(slot))
        );
    }

    /// Zone snapping is a departure from the window's own geometry in exactly
    /// the sense maximizing and half-snapping are, so it goes through the same
    /// `restore_rect` bookkeeping and is undone by the same `restore_window`.
    ///
    /// The mixed sequence is the point: each transition is a chance to
    /// overwrite `restore_rect` with geometry the window only has because of
    /// the previous one, and a zone snap that skipped the check would leave the
    /// window unable ever to get back to its original size.
    #[test]
    fn zone_snapping_restores_to_the_windows_own_geometry() {
        let (mut comp, id) = with_one_window();
        let before = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        comp.maximize_window(id).expect("maximize");
        comp.snap_window_to_zone(
            id,
            SnapSlot::new(SnapLayoutPreset::FourQuadrants, 2).expect("slot"),
        )
        .expect("snap to zone");
        assert_ne!(
            Rect::new(
                comp.window_ref(id).expect("window").x,
                comp.window_ref(id).expect("window").y,
                comp.window_ref(id).expect("window").width,
                comp.window_ref(id).expect("window").height,
            ),
            before,
            "the zone snap moved nothing, so the restore proves nothing"
        );
        comp.snap_window(id, SnapEdge::Left).expect("snap left");
        comp.snap_window_to_zone(
            id,
            SnapSlot::new(SnapLayoutPreset::ThreeColumns, 1).expect("slot"),
        )
        .expect("snap to zone again");

        comp.restore_window(id).expect("restore");

        let w = comp.window_ref(id).expect("window");
        assert_eq!(Rect::new(w.x, w.y, w.width, w.height), before);
        assert_eq!(w.snapped, None);
    }

    /// Zone snapping and maximizing are alternatives, not stages — the same
    /// mutual exclusion half-snapping already keeps.
    #[test]
    fn a_window_is_never_zone_snapped_and_maximized_at_once() {
        let (mut comp, id) = with_one_window();
        let slot = SnapSlot::new(SnapLayoutPreset::ThreeLeftTwoRight, 0).expect("slot");

        comp.maximize_window(id).expect("maximize");
        comp.snap_window_to_zone(id, slot).expect("snap to zone");
        assert!(
            !comp.window_ref(id).expect("window").maximized,
            "zone-snapping a maximized window left it recorded as maximized too"
        );

        comp.maximize_window(id).expect("maximize again");
        assert_eq!(
            comp.window_ref(id).expect("window").snapped,
            None,
            "maximizing a zone-snapped window left it recorded as snapped too"
        );
    }

    /// A fixed-size window refuses a zone snap for the reason it refuses every
    /// other tile: tiling it is a resize it said it cannot take.
    #[test]
    fn a_non_resizable_window_refuses_to_be_zone_snapped() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Fixed", 200, 150);
        spec.position = Some((100, 100));
        spec.resizable = false;
        let id = comp.create_window_from_spec(&spec, 1);
        let before = {
            let w = comp.window_ref(id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        assert!(matches!(
            comp.snap_window_to_zone(
                id,
                SnapSlot::new(SnapLayoutPreset::TwoThirdsRight, 1).expect("slot")
            ),
            Err(CompositorError::NotResizable(_))
        ));
        let w = comp.window_ref(id).expect("window");
        assert_eq!(Rect::new(w.x, w.y, w.width, w.height), before);
        assert_eq!(w.snapped, None);
    }

    /// The shell's request reaches the window, end to end: a `SnapToZone`
    /// arriving as a request moves the window and is answered `Ok`.
    ///
    /// Distinct from calling `snap_window_to_zone` directly — this is what
    /// proves the new wire verb is actually dispatched rather than merely
    /// decodable.
    #[test]
    fn the_snap_to_zone_request_reaches_the_window() {
        let (mut comp, id) = with_one_bare_window(800, 600);
        let slot = SnapSlot::new(SnapLayoutPreset::FourQuadrants, 3).expect("slot");
        let expected = zone_rect(
            slot.rect(work_area_of(comp.display_manager.virtual_bounds()))
                .expect("zone"),
        );

        let response = comp.handle_request(CompositorRequest::ShellControl {
            window_id: id,
            action: ShellControlAction::SnapToZone(slot),
        });

        assert!(matches!(response, CompositorResponse::Ok));
        let w = comp.window_ref(id).expect("window");
        assert_eq!(Rect::new(w.x, w.y, w.width, w.height), expected);
        assert_eq!(w.snapped, Some(SnapTarget::Zone(slot)));
    }

    /// Zones that share an edge round to the *same* pixel, so the tiled windows
    /// neither overlap by a column nor leave one bare.
    ///
    /// Rounding each zone's origin and its size independently is the obvious
    /// way to get from the layout's `f32` rectangles to whole pixels, and it is
    /// wrong: a boundary at 666.67 becomes an origin of 667 for the zone on its
    /// right while the zone on its left still ends at 333 + 333 = 666, and the
    /// column at x = 666 belongs to nobody. Deriving the extents from rounded
    /// *edges* is what closes it.
    ///
    /// A work area too short for the zone gap is used because that is the case
    /// in which the layout drops the gap — and a dropped gap is the only time
    /// two zones share an edge at all.
    #[test]
    fn zones_that_share_an_edge_round_to_the_same_pixel() {
        // 1000 / 3 is 333.33: every interior boundary is fractional.
        let area = WorkArea::new(0.0, 0.0, 1000.0, 4.0);
        let rects: Vec<Rect> = (0..3)
            .map(|z| {
                let slot = SnapSlot::new(SnapLayoutPreset::ThreeColumns, z).expect("slot");
                zone_rect(slot.rect(area).expect("zone"))
            })
            .collect();

        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0].x, 0, "the first column starts at the left edge");
        for pair in rects.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            assert_eq!(
                left.x
                    .saturating_add(i32::try_from(left.width).expect("small")),
                right.x,
                "a gap or an overlap between columns {left:?} and {right:?}"
            );
        }
        let last = rects[2];
        assert_eq!(
            last.x
                .saturating_add(i32::try_from(last.width).expect("small")),
            1000,
            "the last column must reach the right edge"
        );
    }

    /// A layout resolves against whatever display it is asked about, because
    /// the slot names no pixels. Snapping to the same slot on a differently
    /// sized display must land somewhere different.
    #[test]
    fn a_zone_is_resolved_against_the_display_it_is_snapped_on() {
        let slot = SnapSlot::new(SnapLayoutPreset::TwoThirdsLeft, 1).expect("slot");

        let (mut small, small_id) = with_one_bare_window(800, 600);
        small.snap_window_to_zone(small_id, slot).expect("snap");
        let on_small = {
            let w = small.window_ref(small_id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        let (mut large, large_id) = with_one_bare_window(1920, 1080);
        large.snap_window_to_zone(large_id, slot).expect("snap");
        let on_large = {
            let w = large.window_ref(large_id).expect("window");
            Rect::new(w.x, w.y, w.width, w.height)
        };

        assert_ne!(
            on_small, on_large,
            "the slot resolved to one rectangle on two different displays, \
             which means a rectangle got stored somewhere it should not have"
        );
    }

    #[test]
    fn a_near_edge_drag_stopped_by_a_minimum_size_does_not_walk_the_window_away() {
        // The near edge must move by the size change that actually happened,
        // not by the one the pointer asked for. Deriving the origin from the
        // *requested* width let a window whose `min_size` refused the shrink
        // keep sliding right under a pointer dragging its left border, growing
        // no smaller and never stopping.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Bounded", 200, 150);
        spec.position = Some((300, 100));
        spec.min_size = Some((180, 100));
        let id = comp.create_window_from_spec(&spec, 1);

        // Ask to shrink the width by 100 from the left; only 20 is available.
        drag_border(&mut comp, id, 299, 175, 100, 0);
        let win = comp.window_ref(id).expect("window");
        assert_eq!(win.width, 180, "the minimum holds");
        assert_eq!(
            win.x, 320,
            "the left edge moved by the 20 px the window actually gave up"
        );
        // The right edge — the one the user is not touching — must not move.
        assert_eq!(win.x.saturating_add(win.width as i32), 500);
    }

    #[test]
    fn a_resize_drag_from_the_coordinate_edge_does_not_overflow() {
        // `start_size as i32 + dx` overflowed for a large window and a distant
        // pointer, and the pointer position is not the compositor's to bound —
        // it arrives from a device or from a client injecting one.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Wide", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);

        comp.handle_mouse_button(MouseButton::Left, true, 300, 175);
        comp.drag = Some(DragState {
            window_id: id,
            mode: DragMode::ResizeBottomRight,
            start_mouse: Point::new(i32::MIN, i32::MIN),
            start_window_size: (u32::MAX, u32::MAX),
            start_window_pos: Point::new(0, 0),
        });
        comp.handle_mouse_move(i32::MAX, i32::MAX);
        let win = comp.window_ref(id).expect("window survived");
        assert_eq!((win.width, win.height), (u32::MAX, u32::MAX));

        // And the same drag run the other way, which shrinks past zero. A near
        // edge moves *against* the pointer delta — dragging the left border
        // rightwards is what makes the window narrower — so the shrinking case
        // is the one where the pointer travels in the positive direction.
        comp.drag = Some(DragState {
            window_id: id,
            mode: DragMode::ResizeTopLeft,
            start_mouse: Point::new(i32::MIN, i32::MIN),
            start_window_size: (10, 10),
            start_window_pos: Point::new(i32::MIN, i32::MIN),
        });
        comp.handle_mouse_move(i32::MAX, i32::MAX);
        let win = comp.window_ref(id).expect("window survived");
        assert_eq!(
            (win.width, win.height),
            (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn a_declared_minimum_size_survives_a_resize_below_it() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Bounded", 400, 300);
        spec.min_size = Some((320, 240));
        spec.max_size = Some((640, 480));
        let id = comp.create_window_from_spec(&spec, 1);

        comp.resize_window(id, 10, 10).expect("resize");
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (320, 240));

        comp.resize_window(id, 5000, 5000).expect("resize");
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (640, 480));
    }

    #[test]
    fn a_spec_smaller_than_its_own_minimum_is_created_at_the_minimum() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Contradictory", 100, 100);
        spec.min_size = Some((300, 200));
        let id = comp.create_window_from_spec(&spec, 1);

        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (300, 200));
    }

    #[test]
    fn a_requested_position_is_honoured_and_an_absent_one_is_chosen() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");

        let mut placed = WindowSpec::new("Placed", 100, 100);
        placed.position = Some((-40, 17));
        let placed_id = comp.create_window_from_spec(&placed, 1);
        let win = comp.window_ref(placed_id).expect("window");
        // Negative coordinates are a legitimate request: a window may be
        // partly off the left edge of the display.
        assert_eq!((win.x, win.y), (-40, 17));

        let floating = comp.create_window("Floating".to_string(), 100, 100, 1);
        let win = comp.window_ref(floating).expect("window");
        assert_ne!((win.x, win.y), (-40, 17));
    }

    #[test]
    fn maximizing_is_refused_for_a_non_resizable_window() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Dialog", 300, 200);
        spec.resizable = false;
        let id = comp.create_window_from_spec(&spec, 1);

        assert!(matches!(
            comp.maximize_window(id),
            Err(CompositorError::NotResizable(_))
        ));
        let win = comp.window_ref(id).expect("window");
        assert!(!win.maximized);
        assert_eq!((win.width, win.height), (300, 200));
    }

    #[test]
    fn an_undecorated_window_maximizes_to_the_whole_display() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Bare", 100, 100);
        spec.decorations = false;
        let id = comp.create_window_from_spec(&spec, 1);

        comp.maximize_window(id).expect("maximize");
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.x, win.y), (0, 0));
        assert_eq!((win.width, win.height), (800, 600));
    }

    #[test]
    fn a_cursor_a_client_asks_for_applies_only_over_that_client() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");

        let mut left = WindowSpec::new("Editor", 200, 150);
        left.position = Some((50, 200));
        let left_id = comp.create_window_from_spec(&left, 1);

        let mut right = WindowSpec::new("Files", 200, 150);
        right.position = Some((400, 200));
        let right_id = comp.create_window_from_spec(&right, 2);

        comp.set_cursor(left_id, CursorShape::Text).expect("cursor");

        // Over the editor: the I-beam it asked for.
        assert_eq!(comp.cursor_at(100, 250), CursorShape::Text);
        // Over the file manager, which asked for nothing: still an arrow. The
        // bug this replaces set one global shape, so the editor's I-beam
        // showed here too.
        assert_eq!(comp.cursor_at(450, 250), CursorShape::Arrow);
        // Over the desktop: an arrow.
        assert_eq!(comp.cursor_at(700, 500), CursorShape::Arrow);

        // And a second client's request does not disturb the first's.
        comp.set_cursor(right_id, CursorShape::Hand)
            .expect("cursor");
        assert_eq!(comp.cursor_at(100, 250), CursorShape::Text);
        assert_eq!(comp.cursor_at(450, 250), CursorShape::Hand);
    }

    #[test]
    fn the_compositors_own_furniture_keeps_its_own_cursor() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Editor", 200, 150);
        spec.position = Some((100, 200));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.set_cursor(id, CursorShape::Text).expect("cursor");

        let win = comp.window_ref(id).expect("window").clone();
        let bar = win.title_bar_rect().expect("decorated");
        // The title bar is the compositor's, not the client's: the client's
        // I-beam must not follow the pointer onto it.
        assert_eq!(comp.cursor_at(bar.x + 4, bar.y + 4), CursorShape::Arrow);
        // The left border is a resize affordance, which outranks everything.
        assert_eq!(comp.cursor_at(win.x - 1, win.y + 10), CursorShape::ResizeEW);
    }

    #[test]
    fn setting_a_cursor_takes_effect_under_the_pointer_without_moving_it() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let mut spec = WindowSpec::new("Editor", 200, 150);
        spec.position = Some((100, 200));
        let id = comp.create_window_from_spec(&spec, 1);

        comp.handle_mouse_move(150, 250);
        assert_eq!(comp.cursor_shape(), CursorShape::Arrow);

        // The pointer is already inside the window; a client switching to an
        // I-beam should not require the user to jiggle the mouse to see it.
        comp.set_cursor(id, CursorShape::Text).expect("cursor");
        assert_eq!(comp.cursor_shape(), CursorShape::Text);
    }

    #[test]
    fn hiding_a_window_moves_focus_off_it() {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let under = comp.create_window("Under".to_string(), 100, 100, 1);
        let over = comp.create_window("Over".to_string(), 100, 100, 1);
        assert_eq!(comp.focused_window, Some(over));

        comp.set_visible(over, false).expect("hide");
        assert!(!comp.window_ref(over).expect("window").visible);
        assert_eq!(comp.focused_window, Some(under));

        // Showing it again un-minimizes: the client asked for it on screen.
        comp.minimize_window(under).expect("minimize");
        comp.set_visible(under, true).expect("show");
        let win = comp.window_ref(under).expect("window");
        assert!(win.visible && !win.minimized);
    }

    #[test]
    fn test_frame_stats() {
        let mut stats = FrameStats::new(Duration::from_millis(16));
        assert!(stats.should_compose());

        stats.begin_frame();
        let within_budget = stats.end_frame();
        assert!(within_budget);
        assert_eq!(stats.frames_composited, 1);
        assert_eq!(stats.dropped_frames, 0);
    }

    #[test]
    fn test_display_manager() {
        let dm = DisplayManager::new(1920, 1080, 144);
        assert_eq!(dm.displays().len(), 1);
        assert_eq!(dm.primary_refresh_rate(), 144);

        let bounds = dm.virtual_bounds();
        assert_eq!(bounds.width, 1920);
        assert_eq!(bounds.height, 1080);
    }

    #[test]
    fn test_multi_monitor() {
        let mut dm = DisplayManager::new(1920, 1080, 60);
        dm.add_display(Display::new(1, 2560, 1440, 144, 1.5, false));

        assert_eq!(dm.displays().len(), 2);
        let bounds = dm.virtual_bounds();
        // Second display should be placed to the right.
        assert_eq!(bounds.width, 1920 + 2560);
        assert_eq!(bounds.height, 1440); // Max height
    }

    // Exact equality is the correct assertion here, not a tolerance: 0.0, 0.5
    // and 1.0 are all exactly representable, and `set_opacity` only clamps —
    // it does no arithmetic that could round. A tolerance would weaken the
    // test into passing if clamping ever landed slightly off the endpoint.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_opacity() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Ghost".to_string(), 200, 150, 1);

        assert!(comp.set_opacity(id, 0.5).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 0.5);

        // Clamp to valid range.
        assert!(comp.set_opacity(id, 2.0).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 1.0);

        assert!(comp.set_opacity(id, -1.0).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 0.0);
    }

    #[test]
    fn test_window_not_found_error() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let fake_id = WindowId(99999);

        assert!(comp.destroy_window(fake_id).is_err());
        assert!(comp.move_window(fake_id, 0, 0).is_err());
        assert!(comp.resize_window(fake_id, 100, 100).is_err());
    }

    #[test]
    fn test_render_commands_execution() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Render".to_string(), 200, 150, 1);

        let commands = vec![
            RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                color: Color::RED,
                corner_radii: CornerRadii::ZERO,
            },
            RenderCommand::Text {
                x: 10.0,
                y: 10.0,
                text: "Test".to_string(),
                color: Color::WHITE,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
            RenderCommand::Line {
                x1: 0.0,
                y1: 50.0,
                x2: 200.0,
                y2: 50.0,
                color: Color::BLACK,
                width: 1.0,
            },
        ];

        assert!(comp.submit_render(id, commands).is_ok());
        // Compose should succeed with the submitted content.
        assert!(comp.compose_frame());
    }

    #[test]
    fn test_first_command_covers_client() {
        // Opaque, square-cornered, full-cover FillRect at a fully-opaque window
        // => the white background fill can be skipped.
        let full = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(Compositor::first_command_covers_client(
            &full, 200, 150, 1.0
        ));
        // Overshooting origin/size still counts as full cover.
        let overshoot = vec![RenderCommand::FillRect {
            x: -5.0,
            y: -5.0,
            width: 300.0,
            height: 300.0,
            color: Color::rgba(0, 0, 0, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(Compositor::first_command_covers_client(
            &overshoot, 200, 150, 1.0
        ));

        // Translucent window => must NOT skip (top rect would blend).
        assert!(!Compositor::first_command_covers_client(
            &full, 200, 150, 0.5
        ));
        // Non-opaque color => must NOT skip.
        let translucent = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 128),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(!Compositor::first_command_covers_client(
            &translucent,
            200,
            150,
            1.0
        ));
        // Rounded corners => must NOT skip (corner pixels show background).
        let rounded = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::all(8.0),
        }];
        assert!(!Compositor::first_command_covers_client(
            &rounded, 200, 150, 1.0
        ));
        // Partial-cover rect => must NOT skip.
        let partial = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(!Compositor::first_command_covers_client(
            &partial, 200, 150, 1.0
        ));
        // First command is not a FillRect => must NOT skip.
        let text_first = vec![RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: "hi".to_string(),
            color: Color::WHITE,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        }];
        assert!(!Compositor::first_command_covers_client(
            &text_first,
            200,
            150,
            1.0
        ));
        // Empty command list => must NOT skip.
        assert!(!Compositor::first_command_covers_client(&[], 200, 150, 1.0));
    }

    /// Benchmark: full-desktop recomposite cost at 4K (3840x2160).
    ///
    /// CLAUDE.md's performance-critical-subsystems table requires the
    /// compositor to "composite a full desktop in < 2ms at 4K to not miss
    /// 144Hz vsync". This measures the raw full-recomposite cost — clear the
    /// 4K back buffer, redraw every decorated window with toolkit-style client
    /// content, and swap — via `bench_full_composite`, which bypasses the
    /// vsync frame-rate gate (a tight loop over `compose_frame` would be
    /// throttled by `should_compose` and skip most iterations).
    ///
    /// Run it explicitly (it is `#[ignore]`d so it never slows the normal
    /// correctness run) on a RELEASE build — the debug build's unoptimised
    /// per-pixel loops are not representative:
    ///
    /// ```text
    /// cargo test -p compositor --target x86_64-pc-windows-gnu --release \
    ///   -- --ignored --nocapture bench_compose_frame_4k
    /// ```
    ///
    /// The `< 2ms/4K` target is judged on a release build (ideally on real
    /// hardware); the recorded dev-host baseline lives in
    /// `bench/baselines.toml` under `[compositor_frame_4k]`. As of 2026-07-02
    /// the compositor is still over target (~15.8ms/frame release on the dev host
    /// after the row-wise `fill_rect` rewrite + redundant-bg-fill occlusion cull,
    /// down from ~48.6ms — see known-issues BENCH-COMPOSITOR-SLOW; the remaining
    /// gap is memory-bandwidth bound on a full recomposite). This test therefore
    /// does NOT assert the 2ms target (it would always fail on a full-recomposite
    /// stress).
    /// It prints a PASS/OVER verdict for tracking and hard-fails only on a
    /// catastrophic regression (mean > 150 ms/frame, ~3x the current baseline)
    /// so an accidental super-linear blow-up is still caught without flaking.
    #[test]
    #[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
    fn bench_compose_frame_4k() {
        const W: u32 = 3840;
        const H: u32 = 2160;
        const NUM_WINDOWS: usize = 16;
        const WARMUP: usize = 5;
        const ITERS: usize = 60;
        const TARGET_MS: f64 = 2.0;

        let mut comp = Compositor::new(W, H, 144).expect("4K compositor");

        // A representative desktop: overlapping decorated windows, each
        // carrying toolkit-style client content (a titlebar band, a large
        // content panel, and a text label), cascaded across the screen.
        for i in 0..NUM_WINDOWS {
            let ww = 1100u32;
            let wh = 720u32;
            let id = comp.create_window(format!("Window {i}"), ww, wh, i as u64 + 1);
            let step = i as i32;
            comp.move_window(id, 60 + step * 170, 40 + step * 110)
                .expect("move_window");
            let commands = vec![
                RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: ww as f32,
                    height: wh as f32,
                    color: Color::rgba(30, 34, 40, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::FillRect {
                    x: 20.0,
                    y: 20.0,
                    width: (ww - 40) as f32,
                    height: 80.0,
                    color: Color::rgba(60, 120, 200, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::FillRect {
                    x: 20.0,
                    y: 120.0,
                    width: (ww - 40) as f32,
                    height: (wh - 160) as f32,
                    color: Color::rgba(45, 48, 54, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::Text {
                    x: 30.0,
                    y: 40.0,
                    text: format!("Panel {i}"),
                    color: Color::WHITE,
                    font_size: 18.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                },
            ];
            comp.submit_render(id, commands).expect("submit_render");
        }

        // Warm up: page in both framebuffers, prime caches/predictors.
        for _ in 0..WARMUP {
            comp.bench_full_composite();
        }

        let mut min_ns = u64::MAX;
        let mut total_ns = 0u64;
        for _ in 0..ITERS {
            let start = std::time::Instant::now();
            comp.bench_full_composite();
            let ns = start.elapsed().as_nanos() as u64;
            min_ns = min_ns.min(ns);
            total_ns = total_ns.saturating_add(ns);
        }
        let mean_ns = total_ns / ITERS as u64;
        let min_ms = min_ns as f64 / 1_000_000.0;
        let mean_ms = mean_ns as f64 / 1_000_000.0;
        let verdict = if min_ms <= TARGET_MS { "PASS" } else { "OVER" };
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };

        println!(
            "[compositor-bench] compose_frame 4K ({W}x{H}, {NUM_WINDOWS} windows, \
             {profile} build): min={min_ms:.3}ms mean={mean_ms:.3}ms  \
             target<{TARGET_MS}ms => {verdict}  (target judged on release+hardware)"
        );

        // Phase split: the aggregate cannot say which half to optimize.
        let (mut cmin, mut wmin) = (u64::MAX, u64::MAX);
        for _ in 0..ITERS {
            let (c, w) = comp.bench_full_composite_phases();
            cmin = cmin.min(c);
            wmin = wmin.min(w);
        }
        println!(
            "[compositor-bench] phases (min): background_clear={:.3}ms window_render={:.3}ms",
            cmin as f64 / 1_000_000.0,
            wmin as f64 / 1_000_000.0
        );

        // Catastrophic-regression guard only (see doc): the current baseline
        // is ~16ms (still over the 2ms target, tracked separately); a mean past
        // 80ms (~5x the baseline, and worse than the pre-optimization ~50ms)
        // means a super-linear blow-up crept into the path.
        assert!(
            mean_ms < 80.0,
            "compositor 4K recomposite mean {mean_ms:.3}ms is a catastrophic regression (>80ms)"
        );
    }

    /// Sum of the areas of a rect list, as a `u64` so it cannot overflow.
    fn total_area(rects: &[Rect]) -> u64 {
        rects.iter().map(|r| r.width as u64 * r.height as u64).sum()
    }

    /// Every pixel of `outer` that is in exactly one of `parts`, brute-forced.
    ///
    /// Deliberately naive: it is the independent oracle the fast rectangle
    /// algebra is checked against, so it must not share any of its reasoning.
    fn coverage_count(outer: Rect, parts: &[Rect], px: i32, py: i32) -> usize {
        let _ = outer;
        parts.iter().filter(|r| r.contains(px, py)).count()
    }

    #[test]
    fn subtract_yields_disjoint_parts_that_miss_exactly_the_occluder() {
        let base = Rect::new(-3, -2, 20, 14);
        // A cut from every direction, plus the degenerate ones.
        let occluders = [
            Rect::new(0, 0, 5, 5),         // interior-ish corner
            Rect::new(5, 3, 4, 4),         // strictly interior -> 4 parts
            Rect::new(-100, -100, 1, 1),   // disjoint
            Rect::new(-50, -50, 200, 200), // covers everything
            Rect::new(-3, -2, 20, 3),      // full-width band at the top
            Rect::new(10, -2, 7, 14),      // full-height band at the right
        ];
        for occ in occluders {
            let parts = base.subtract(&occ);
            // Exhaustive over the base rect and a one-pixel margin, so a part
            // that strayed outside would be caught too.
            for py in (base.y - 1)..=base.bottom() {
                for px in (base.x - 1)..=base.right() {
                    let want = usize::from(base.contains(px, py) && !occ.contains(px, py));
                    let got = coverage_count(base, &parts, px, py);
                    assert_eq!(
                        got, want,
                        "occluder {occ:?} at ({px},{py}): covered {got} times, wanted {want}"
                    );
                }
            }
            // Areas must add up exactly — the check that catches a part that is
            // disjoint and yet still the wrong size.
            let want_area = base.width as u64 * base.height as u64
                - base
                    .intersect(&occ)
                    .map_or(0, |i| i.width as u64 * i.height as u64);
            assert_eq!(
                total_area(&parts),
                want_area,
                "area after subtracting {occ:?}"
            );
        }
    }

    #[test]
    fn a_union_across_the_whole_coordinate_space_does_not_overflow() {
        // `(x2 - x1) as u32` used to compute the width here. Both coordinates
        // come off the wire — `CreateWindow` and `SetPosition` carry a client's
        // chosen `i32` position — so a client could place one window at each end
        // of the coordinate space and panic the display server with the damage
        // union of the two. The distance does not fit in an `i32` at all, which
        // is why the fix computes the extent directly rather than subtracting.
        let far_left = Rect::new(i32::MIN, i32::MIN, 10, 10);
        let far_right = Rect::new(i32::MAX - 10, i32::MAX - 10, 10, 10);
        let both = far_left.union(&far_right);
        assert_eq!(both.x, i32::MIN);
        assert_eq!(both.y, i32::MIN);
        assert_eq!(both.width, u32::MAX);
        assert_eq!(both.height, u32::MAX);

        // And the ordinary case still gives the ordinary answer.
        let a = Rect::new(-3, -2, 20, 14);
        let b = Rect::new(5, 5, 4, 4);
        assert_eq!(a.union(&b), Rect::new(-3, -2, 20, 14));
        assert_eq!(
            Rect::new(0, 0, 5, 5).union(&Rect::new(10, 10, 5, 5)),
            Rect::new(0, 0, 15, 15)
        );
    }

    #[test]
    fn span_is_exact_at_the_edges_of_the_coordinate_space() {
        // The whole coordinate space is `u32::MAX` wide, exactly — which is the
        // reason the answer cannot be computed by subtracting in `i32`.
        assert_eq!(span(i32::MIN, i32::MAX), u32::MAX);
        assert_eq!(span(i32::MIN, 0), 1 << 31);
        assert_eq!(span(0, i32::MAX), i32::MAX as u32);
        // Not past `lo` is an empty span, not an error: two rectangles that
        // merely touch produce one.
        assert_eq!(span(5, 5), 0);
        assert_eq!(span(5, 4), 0);
        assert_eq!(span(i32::MAX, i32::MIN), 0);
        assert_eq!(span(-7, 3), 10);
    }

    #[test]
    fn a_window_at_the_coordinate_edge_keeps_its_frame_on_the_screen() {
        // The frame is *added* to a client-chosen position and size, so both
        // ends can overflow. Saturating pins the window at the extreme; the bug
        // it replaces would have wrapped it to the far side of the desktop.
        let w = plain_window(i32::MIN, i32::MIN, 100, 100);
        assert!(w.is_framed(), "the case only arises for a framed window");
        let outer = w.outer_rect();
        assert_eq!(outer.x, i32::MIN);
        assert_eq!(outer.y, i32::MIN);

        let huge = plain_window(0, 0, u32::MAX, u32::MAX);
        let outer = huge.outer_rect();
        assert_eq!(outer.width, u32::MAX);
        assert_eq!(outer.height, u32::MAX);
        let title = huge.title_bar_rect().expect("a decorated window has one");
        assert_eq!(title.width, u32::MAX);
    }

    #[test]
    fn subtract_region_declines_rather_than_fragmenting_without_bound() {
        let base = Rect::new(0, 0, 100, 100);
        // Three strictly-interior holes cannot be expressed in four rects.
        let many = [
            Rect::new(10, 10, 5, 5),
            Rect::new(30, 30, 5, 5),
            Rect::new(50, 50, 5, 5),
        ];
        assert!(
            subtract_region(base, &many, 4).is_none(),
            "should decline rather than return a huge fragment list"
        );
        // A single full-width band stays cheap, and is not declined.
        let band = [Rect::new(0, 0, 100, 40)];
        let parts = subtract_region(base, &band, 4).expect("one band is one part");
        assert_eq!(parts, vec![Rect::new(0, 40, 100, 60)]);
        // Fully covered is an empty region, not a decline.
        let all = [Rect::new(-1, -1, 200, 200)];
        assert_eq!(subtract_region(base, &all, 4), Some(vec![]));
    }

    /// The property the whole occlusion cull rests on: it must not change a
    /// single pixel of the composited image.
    #[test]
    fn occlusion_cull_composites_the_same_pixels_as_drawing_every_window() {
        // Small enough to compare exhaustively, cascaded like the 4K benchmark
        // so windows really do occlude one another.
        const W: u32 = 900;
        const H: u32 = 700;

        let build = |cull: bool| {
            let mut comp = Compositor::new(W, H, 60).expect("compositor");
            comp.occlusion_cull = cull;
            for i in 0..6usize {
                let (ww, wh) = (380u32, 260u32);
                let id = comp.create_window(format!("W{i}"), ww, wh, i as u64 + 1);
                let step = i as i32;
                comp.move_window(id, 40 + step * 90, 60 + step * 70)
                    .expect("move_window");
                // Window 3 is deliberately translucent: a translucent window is
                // neither an occluder nor safely double-drawable, so it is the
                // one that would expose a non-disjoint fragmentation.
                if i == 3 {
                    comp.set_opacity(id, 0.5).expect("opacity");
                }
                comp.submit_render(
                    id,
                    vec![
                        RenderCommand::FillRect {
                            x: 0.0,
                            y: 0.0,
                            width: ww as f32,
                            height: wh as f32,
                            color: Color::rgba(30, 34, 40, 255),
                            corner_radii: CornerRadii::ZERO,
                        },
                        RenderCommand::FillRect {
                            x: 15.0,
                            y: 15.0,
                            width: (ww - 30) as f32,
                            height: 50.0,
                            color: Color::rgba(60, 120, 200, 255),
                            corner_radii: CornerRadii::ZERO,
                        },
                        RenderCommand::Text {
                            x: 20.0,
                            y: 25.0,
                            text: format!("Panel {i}"),
                            color: Color::WHITE,
                            font_size: 16.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                            overflow: TextOverflow::Clip,
                        },
                    ],
                )
                .expect("submit_render");
            }
            comp.bench_full_composite();
            comp.backend.presented_pixels().to_vec()
        };

        let culled = build(true);
        let reference = build(false);
        assert_eq!(culled.len(), reference.len(), "framebuffer sizes differ");

        let diffs: Vec<usize> = culled
            .iter()
            .zip(reference.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .take(8)
            .collect();
        assert!(
            diffs.is_empty(),
            "occlusion cull changed {} pixel(s); first few at (x,y): {:?}",
            culled
                .iter()
                .zip(reference.iter())
                .filter(|(a, b)| a != b)
                .count(),
            diffs
                .iter()
                .map(|i| (i % W as usize, i / W as usize))
                .collect::<Vec<_>>()
        );
    }

    /// Build `w*h` tightly-packed (stride = w*4) ARGB bytes all of `color`.
    fn solid_buffer_bytes(w: u32, h: u32, color: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn test_attach_buffer_blits_pixels() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        // First window: client area lands at (100, 80).
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);

        let color = 0xFF11_2233u32;
        let bytes = solid_buffer_bytes(4, 4, color);
        assert!(
            comp.attach_buffer(id, 55, 4, 4, 16, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );
        assert!(comp.compose_frame());

        // The 4x4 buffer should have been blitted at the client origin (100,80).
        let front = comp.backend.presented_pixels();
        let stride = 400usize;
        assert_eq!(front[80 * stride + 100], color, "buffer top-left pixel");
        assert_eq!(front[83 * stride + 103], color, "buffer bottom-right pixel");
        // Beyond the 4x4 buffer (within the 8x8 client area) is NOT buffer
        // content — the client owns its surface, uncovered area stays cleared.
        assert_ne!(front[80 * stride + 106], color, "outside buffer extent");
    }

    #[test]
    fn test_attach_buffer_rejects_bad_geometry() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);

        // Stride too small for width 4 (needs 16 bytes/row).
        let bytes = vec![0u8; 64];
        assert!(matches!(
            comp.attach_buffer(id, 1, 4, 4, 8, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::InvalidBuffer(_))
        ));
        // The failed attach must leave the window on the render-tree path.
        assert!(comp.window_ref(id).unwrap().buffer.is_none());
    }

    #[test]
    fn test_attach_buffer_window_not_found() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let bytes = solid_buffer_bytes(2, 2, 0xFFFFFFFF);
        assert!(matches!(
            comp.attach_buffer(WindowId(424242), 1, 2, 2, 8, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::WindowNotFound(_))
        ));
    }

    #[test]
    fn test_detach_buffer_reverts_to_commands() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0xFF00FF00);

        assert!(
            comp.attach_buffer(id, 77, 4, 4, 16, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );
        assert!(comp.window_ref(id).unwrap().buffer.is_some());

        assert_eq!(comp.detach_buffer(id), Some(77));
        assert!(comp.window_ref(id).unwrap().buffer.is_none());
        // Detaching again returns None (nothing attached).
        assert_eq!(comp.detach_buffer(id), None);
    }

    #[test]
    fn test_buffer_release_notification() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0xFF334455);

        assert!(
            comp.attach_buffer(id, 0xABCD, 4, 4, 16, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );
        // Before compositing, nothing has been read yet.
        assert!(comp.take_released_buffer_handles().is_empty());

        assert!(comp.compose_frame());
        // After compositing, the buffer is released exactly once.
        assert_eq!(comp.take_released_buffer_handles(), vec![0xABCD]);
        assert!(comp.take_released_buffer_handles().is_empty());
    }

    #[test]
    fn test_copy_row_clips_left_right_and_vertical() {
        let mut fb = Framebuffer::new(4, 2).unwrap();
        fb.clear(0xFF000000);
        let src = [0xFFAAAAAA, 0xFFBBBBBB, 0xFFCCCCCC, 0xFFDDDDDD, 0xFFEEEEEE];

        // Partly offscreen on the left: x=-2 skips the first two src pixels,
        // writes the remaining 3 at columns 0..3.
        fb.copy_row(-2, 0, &src);
        assert_eq!(fb.get_pixel(0, 0), Some(0xFFCCCCCC));
        assert_eq!(fb.get_pixel(1, 0), Some(0xFFDDDDDD));
        assert_eq!(fb.get_pixel(2, 0), Some(0xFFEEEEEE));
        assert_eq!(fb.get_pixel(3, 0), Some(0xFF000000)); // untouched

        // Partly offscreen on the right: x=2 writes only 2 of 5 (cols 2,3).
        fb.clear(0xFF000000);
        fb.copy_row(2, 1, &src);
        assert_eq!(fb.get_pixel(2, 1), Some(0xFFAAAAAA));
        assert_eq!(fb.get_pixel(3, 1), Some(0xFFBBBBBB));
        assert_eq!(fb.get_pixel(0, 1), Some(0xFF000000));

        // Out-of-range y or fully-offscreen x is a no-op.
        fb.clear(0xFF121212);
        fb.copy_row(0, 5, &src);
        fb.copy_row(-10, 0, &src);
        fb.copy_row(4, 0, &src);
        assert_eq!(fb.get_pixel(0, 0), Some(0xFF121212));
    }

    #[test]
    fn test_opaque_buffer_fast_path_matches_blend() {
        // An Xrgb (opaque) buffer at full opacity must blit bit-identically to
        // the per-pixel blend path: blend writes `src | 0xFF000000` and Xrgb
        // import already forced 0xFF alpha, so the fast copy is exact.
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0x0011_2233); // alpha 0x00 in source
        assert!(
            comp.attach_buffer(id, 1, 4, 4, 16, BufferFormat::Xrgb8888, &bytes)
                .is_ok()
        );
        assert!(comp.compose_frame());

        let front = comp.backend.presented_pixels();
        let stride = 400usize;
        // First window's client area lands at (100, 80) by default placement;
        // read via the window's actual client position to stay robust.
        let (wx, wy) = {
            let w = comp.window_ref(id).unwrap();
            (w.x as usize, w.y as usize)
        };
        assert_eq!(
            front[wy * stride + wx],
            0xFF11_2233,
            "opaque fast-path pixel"
        );
        assert_eq!(front[(wy + 3) * stride + wx + 3], 0xFF11_2233);
    }

    #[test]
    fn test_fullscreen_sets_geometry_and_clears() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 200, 150, 1);
        assert!(!comp.is_fullscreen(id));

        assert!(comp.set_fullscreen(id, true).is_ok());
        assert!(comp.is_fullscreen(id));
        {
            let win = comp.window_ref(id).unwrap();
            assert_eq!((win.x, win.y), (0, 0));
            assert_eq!((win.width, win.height), (400, 300));
            assert!(win.fullscreen);
        }

        // Leaving fullscreen restores the original client geometry.
        assert!(comp.set_fullscreen(id, false).is_ok());
        assert!(!comp.is_fullscreen(id));
        let win = comp.window_ref(id).unwrap();
        assert_eq!((win.width, win.height), (200, 150));
    }

    #[test]
    fn test_direct_scanout_bypass_presents_buffer() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());

        // A display-sized opaque buffer makes the window scanout-eligible.
        let color = 0xFF_AB_CD_EFu32;
        let bytes = solid_buffer_bytes(64, 48, color);
        assert!(
            comp.attach_buffer(id, 9, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );

        assert!(comp.compose_frame());
        // Frame should have bypassed compositing entirely.
        assert!(comp.is_scanout_bypassed());
        assert_eq!(comp.scanout(), Scanout::Direct(id));
        assert_eq!(comp.frame_stats.bypass_frames, 1);

        // Presented pixels come straight from the client buffer (zero copy).
        let present = comp.present_pixels();
        assert_eq!(present.len(), 64 * 48);
        assert_eq!(present[0], color);
        assert_eq!(present[64 * 48 - 1], color);

        // The buffer is released exactly once for reuse.
        assert_eq!(comp.take_released_buffer_handles(), vec![9]);
    }

    #[test]
    fn test_no_bypass_when_buffer_smaller_than_display() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());

        // Buffer smaller than the display must NOT bypass (would leave the rest
        // of the screen stale); the compositor falls back to compositing.
        let bytes = solid_buffer_bytes(32, 24, 0xFF112233);
        assert!(
            comp.attach_buffer(id, 1, 32, 24, 32 * 4, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );

        assert!(comp.compose_frame());
        assert!(!comp.is_scanout_bypassed());
        assert_eq!(comp.scanout(), Scanout::Composited);
        // Composited path presents the framebuffer front buffer.
        assert_eq!(comp.present_pixels().len(), 64 * 48);
    }

    #[test]
    fn test_no_bypass_when_not_fullscreen() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Win".to_string(), 64, 48, 1);
        // Display-sized buffer but the window is NOT fullscreen → no bypass.
        let bytes = solid_buffer_bytes(64, 48, 0xFF445566);
        assert!(
            comp.attach_buffer(id, 1, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );
        assert!(comp.compose_frame());
        assert!(!comp.is_scanout_bypassed());
    }

    #[test]
    fn test_no_bypass_when_translucent() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());
        comp.set_opacity(id, 0.5).ok();
        let bytes = solid_buffer_bytes(64, 48, 0xFF778899);
        assert!(
            comp.attach_buffer(id, 1, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
                .is_ok()
        );
        assert!(comp.compose_frame());
        // A translucent fullscreen window must blend with what's beneath, so it
        // cannot be scanned out directly.
        assert!(!comp.is_scanout_bypassed());
    }

    #[test]
    fn test_stream_ipc_lifecycle() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();

        // Start a stream via IPC.
        let stream_id = match comp.handle_request(CompositorRequest::StreamStart) {
            CompositorResponse::StreamStarted { stream_id } => stream_id,
            other => panic!("expected StreamStarted, got {other:?}"),
        };
        assert_eq!(comp.stream_session_count(), 1);

        // Capture produces a decodable wire frame.
        let data = match comp.handle_request(CompositorRequest::StreamCapture { stream_id }) {
            CompositorResponse::StreamFrame { data } => data,
            other => panic!("expected StreamFrame, got {other:?}"),
        };
        let (frame, used) =
            guiremote::scene::decode_scene_frame(&data).expect("decode captured frame");
        assert_eq!(used, data.len(), "the frame must account for all its bytes");
        assert_eq!(frame.sequence, 0);
        assert_eq!(frame.display_width, 400);
        assert_eq!(frame.display_height, 300);

        // Stop frees the session; a second stop reports an error.
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamStop { stream_id }),
            CompositorResponse::Ok
        ));
        assert_eq!(comp.stream_session_count(), 0);
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamStop { stream_id }),
            CompositorResponse::Error { .. }
        ));
        // Capturing a stopped session is an error, not a panic.
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamCapture { stream_id }),
            CompositorResponse::Error { .. }
        ));
    }

    #[test]
    fn test_stream_capture_forwards_window_commands() {
        let mut comp = Compositor::new(200, 150, 60).unwrap();
        let id = comp.create_window("Streamed".to_string(), 100, 80, 1);
        let commands = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 30.0,
            color: Color::RED,
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(comp.submit_render(id, commands).is_ok());

        let stream_id = comp.start_stream();

        // First capture: the window is new to the session → commands present.
        let (f0, _) =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert_eq!(f0.windows.len(), 1);
        assert_eq!(f0.windows[0].id, id.raw());
        let cmds = f0.windows[0]
            .commands
            .as_ref()
            .expect("new window forwards commands");
        assert_eq!(cmds.commands.len(), 1);

        // Second capture with unchanged content: geometry-only delta.
        let (f1, _) =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert_eq!(f1.sequence, 1);
        assert!(f1.windows[0].commands.is_none());

        // Destroying the window makes the next frame report it as removed.
        assert!(comp.destroy_window(id).is_ok());
        let (f2, _) =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert!(f2.windows.is_empty());
        assert_eq!(f2.removed, vec![id.raw()]);
    }

    #[test]
    fn test_clip_stack() {
        let mut stack = ClipStack::default();
        stack.push(Rect::new(0, 0, 100, 100));
        assert_eq!(stack.current(), Some(&Rect::new(0, 0, 100, 100)));

        // Pushing a smaller clip should intersect.
        stack.push(Rect::new(50, 50, 100, 100));
        assert_eq!(stack.current(), Some(&Rect::new(50, 50, 50, 50)));

        stack.pop();
        assert_eq!(stack.current(), Some(&Rect::new(0, 0, 100, 100)));

        stack.pop();
        assert_eq!(stack.current(), None);
    }

    #[test]
    fn test_translate_stack() {
        let mut stack = TranslateStack::default();
        stack.push(10.0, 20.0);
        assert_eq!(stack.offset(), (10.0, 20.0));

        stack.push(5.0, 3.0);
        assert_eq!(stack.offset(), (15.0, 23.0));

        stack.pop();
        assert_eq!(stack.offset(), (10.0, 20.0));

        stack.pop();
        assert_eq!(stack.offset(), (0.0, 0.0));
    }

    // ---- clear_except (occlusion-culled desktop clear) ---------------------

    #[test]
    fn test_clear_except_empty_covered_fills_all() {
        // No covered rects => behaves exactly like `clear`.
        let mut fb = Framebuffer::new(6, 4).unwrap();
        fb.set_pixel(3, 2, 0xFF_12_34_56); // dirty the back buffer first
        fb.clear_except(0xFF_AA_BB_CC, &[]);
        for y in 0..4 {
            for x in 0..6 {
                assert_eq!(fb.get_pixel(x, y), Some(0xFF_AA_BB_CC));
            }
        }
    }

    #[test]
    fn test_clear_except_single_rect_preserves_covered() {
        // A single covered rect: pixels inside keep their prior value, pixels
        // outside get the clear color.
        let mut fb = Framebuffer::new(8, 6).unwrap();
        fb.clear(0xFF_00_00_00); // known prior state everywhere
        let covered = [Rect::new(2, 1, 3, 2)]; // x:2..5, y:1..3
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        for y in 0..6 {
            for x in 0..8 {
                let inside = (2..5).contains(&x) && (1..3).contains(&y);
                let expect = if inside { 0xFF_00_00_00 } else { 0xFF_FF_FF_FF };
                assert_eq!(
                    fb.get_pixel(x, y),
                    Some(expect),
                    "pixel ({x},{y}) inside={inside}"
                );
            }
        }
    }

    #[test]
    fn test_clear_except_overlapping_rects_merge_spans() {
        // Two overlapping rects on the same rows must merge into one covered
        // span (no clear color bleeds into the overlap or the seam between
        // them). Covered union on rows 0..3: x in 1..7.
        let mut fb = Framebuffer::new(10, 4).unwrap();
        fb.clear(0xFF_00_00_00);
        let covered = [Rect::new(1, 0, 4, 3), Rect::new(4, 0, 3, 3)]; // 1..5 and 4..7
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        for y in 0..4 {
            for x in 0..10 {
                let inside = (1..7).contains(&x) && (0..3).contains(&y);
                let expect = if inside { 0xFF_00_00_00 } else { 0xFF_FF_FF_FF };
                assert_eq!(fb.get_pixel(x, y), Some(expect), "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn test_clear_except_clips_offscreen_and_oversized_rects() {
        // Rects extending past the framebuffer edges (and with negative
        // origins) must be clipped, never panic or write OOB.
        let mut fb = Framebuffer::new(6, 5).unwrap();
        fb.clear(0xFF_00_00_00);
        // Straddles the top-left corner and overshoots bottom-right.
        let covered = [Rect::new(-3, -2, 100, 100)];
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        // The whole framebuffer is covered by the clipped rect => nothing gets
        // the clear color.
        for y in 0..5 {
            for x in 0..6 {
                assert_eq!(fb.get_pixel(x, y), Some(0xFF_00_00_00), "pixel ({x},{y})");
            }
        }

        // A fully-offscreen rect covers nothing => full clear.
        let mut fb2 = Framebuffer::new(6, 5).unwrap();
        fb2.clear(0xFF_00_00_00);
        fb2.clear_except(0xFF_FF_FF_FF, &[Rect::new(50, 50, 4, 4)]);
        for y in 0..5 {
            for x in 0..6 {
                assert_eq!(fb2.get_pixel(x, y), Some(0xFF_FF_FF_FF), "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn test_clear_except_parallel_band_boundaries() {
        // A framebuffer above the parallel-fill threshold (>1M px) so the
        // multi-threaded row-band path runs on a multicore host. A covered rect
        // whose vertical extent straddles band boundaries must be skipped
        // correctly in every band (the key parallel-correctness risk), and the
        // result must be bit-identical to a single-threaded clear_except.
        const W: u32 = 2048;
        const H: u32 = 1024; // 2M px > 1<<20 threshold
        let covered = [
            Rect::new(100, 50, 400, 900),   // tall: crosses many band boundaries
            Rect::new(1500, 300, 400, 200), // offset block
        ];

        let mut par = Framebuffer::new(W, H).unwrap();
        par.clear(0xFF_00_00_00);
        par.clear_except(0xFF_AB_CD_EF, &covered);

        // Ground truth: fill the same buffer single-threaded via the shared helper.
        let mut reference = vec![0xFF_00_00_00u32; (W * H) as usize];
        Framebuffer::fill_uncovered_band(&mut reference, 0, H, W, 0xFF_AB_CD_EF, &covered, H);

        assert_eq!(par.back.len(), reference.len());
        assert!(
            par.back == reference,
            "parallel clear_except must match single-threaded reference"
        );

        // Spot-check a covered pixel (kept prior) and an uncovered one (cleared),
        // both far from row 0 so at least one band boundary was crossed.
        assert_eq!(par.get_pixel(200, 600), Some(0xFF_00_00_00));
        assert_eq!(par.get_pixel(900, 600), Some(0xFF_AB_CD_EF));
    }

    // ---- blit_opaque parallel row-band blit ---------------------------------

    /// Build an opaque (Xrgb) `SharedBuffer` whose pixel `(x, y)` carries a
    /// deterministic non-black value, so a copy can be verified per-pixel.
    #[cfg(test)]
    fn make_opaque_test_buffer(w: u32, h: u32) -> SharedBuffer {
        let mut bytes = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let off = ((y * w + x) * 4) as usize;
                // Xrgb: bytes are little-endian [B, G, R, X]; distinct per pixel.
                bytes[off] = (x & 0xFF) as u8;
                bytes[off + 1] = (y & 0xFF) as u8;
                bytes[off + 2] = ((x ^ y) & 0xFF) as u8;
                bytes[off + 3] = 0;
            }
        }
        SharedBuffer::import(9, w, h, w * 4, BufferFormat::Xrgb8888, &bytes).expect("import")
    }

    /// Serial reference: exactly the old blit_buffer opaque fast path.
    #[cfg(test)]
    fn blit_opaque_reference(fb: &mut Framebuffer, buf: &SharedBuffer, win_x: i32, win_y: i32) {
        let cols = buf.width();
        let rows = buf.height();
        for row in 0..rows {
            let sy = win_y.saturating_add(row as i32);
            if sy < 0 {
                continue;
            }
            if let Some(src) = buf.row(row) {
                let run = src.get(..cols as usize).unwrap_or(src);
                fb.copy_row(win_x, sy as u32, run);
            }
        }
    }

    #[test]
    fn test_blit_opaque_matches_serial_reference_large() {
        // Above the parallel threshold so the multi-band path runs on multicore.
        // The buffer is larger than 1<<20 px to force blit_opaque to parallelize.
        const W: u32 = 2048;
        const H: u32 = 1024; // 2M px > threshold
        let buf = make_opaque_test_buffer(1200, 900);

        for &(wx, wy) in &[(0i32, 0i32), (100, 200), (-50, -30), (1900, 800), (2000, 0)] {
            let mut par = Framebuffer::new(W, H).unwrap();
            par.clear(0xFF_00_00_00);
            par.blit_opaque(&buf, wx, wy, buf.width(), buf.height());

            let mut reference = Framebuffer::new(W, H).unwrap();
            reference.clear(0xFF_00_00_00);
            blit_opaque_reference(&mut reference, &buf, wx, wy);

            assert!(
                par.back == reference.back,
                "blit_opaque({wx},{wy}) must match serial reference"
            );
        }
    }

    #[test]
    fn test_blit_opaque_clips_edges_small() {
        // Small buffers exercise the single-threaded path plus all clip corners.
        let buf = make_opaque_test_buffer(16, 12);
        for &(wx, wy) in &[
            (0i32, 0i32),
            (-5, -3), // top-left straddle
            (28, 20), // fully offscreen bottom-right
            (-20, 5), // fully offscreen left
            (25, -2), // right-straddle + top-straddle
        ] {
            let mut got = Framebuffer::new(32, 24).unwrap();
            got.clear(0xFF_11_22_33);
            got.blit_opaque(&buf, wx, wy, buf.width(), buf.height());

            let mut want = Framebuffer::new(32, 24).unwrap();
            want.clear(0xFF_11_22_33);
            blit_opaque_reference(&mut want, &buf, wx, wy);

            assert!(
                got.back == want.back,
                "blit_opaque clip mismatch at ({wx},{wy})"
            );
        }
    }

    // ---- opaque_cover_rects (which windows cull the desktop clear) ---------

    #[test]
    fn test_opaque_cover_rects_reports_opaque_command_window() {
        // A full-opacity window whose first command opaquely covers the client
        // area is reported over its whole client rect.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Solid".to_string(), 200, 150, 1);
        comp.move_window(id, 120, 90).unwrap();
        comp.submit_render(
            id,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 150.0,
                color: Color::rgba(30, 34, 40, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();

        let rects = comp.opaque_cover_rects();
        assert_eq!(rects, vec![Rect::new(120, 90, 200, 150)]);
    }

    #[test]
    fn test_opaque_cover_rects_excludes_translucent_and_hidden() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();

        // Translucent window: opaque command but window opacity < 1.0 => excluded.
        let ghost = comp.create_window("Ghost".to_string(), 100, 100, 1);
        comp.submit_render(
            ghost,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();
        comp.set_opacity(ghost, 0.5).unwrap();

        // Minimized window: opaque + full opacity but not visible => excluded.
        let hidden = comp.create_window("Hidden".to_string(), 100, 100, 2);
        comp.submit_render(
            hidden,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();
        comp.minimize_window(hidden).unwrap();

        // Rounded-corner window: corners show background => not a full cover.
        let rounded = comp.create_window("Rounded".to_string(), 100, 100, 3);
        comp.submit_render(
            rounded,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::all(8.0),
            }],
        )
        .unwrap();

        assert!(
            comp.opaque_cover_rects().is_empty(),
            "no window should cull the desktop clear"
        );
    }

    #[test]
    fn test_opaque_cover_rects_buffer_window_uses_covered_subrect() {
        // An Xrgb (opaque) buffer smaller than the client area only covers the
        // sub-rectangle it actually spans, from the client origin.
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 20, 20, 1);
        let (wx, wy) = {
            let w = comp.window_ref(id).unwrap();
            (w.x, w.y)
        };
        let bytes = solid_buffer_bytes(8, 6, 0x0011_2233);
        comp.attach_buffer(id, 1, 8, 6, 8 * 4, BufferFormat::Xrgb8888, &bytes)
            .unwrap();

        let rects = comp.opaque_cover_rects();
        assert_eq!(rects, vec![Rect::new(wx, wy, 8, 6)]);

        // An Argb buffer (not is_opaque) must NOT be reported.
        let id2 = comp.create_window("Argb".to_string(), 20, 20, 2);
        let bytes2 = solid_buffer_bytes(8, 6, 0xFF00_FF00);
        comp.attach_buffer(id2, 2, 8, 6, 8 * 4, BufferFormat::Argb8888, &bytes2)
            .unwrap();
        let id2_pos = {
            let w = comp.window_ref(id2).unwrap();
            Rect::new(w.x, w.y, 8, 6)
        };
        assert!(
            !comp.opaque_cover_rects().contains(&id2_pos),
            "Argb buffer window must not be treated as opaque"
        );
    }

    #[test]
    fn test_full_recomposite_cull_matches_uncovered_background() {
        // Visual-equivalence: after a full recomposite, the desktop background
        // shows through where nothing covers it, and covered pixels carry the
        // window's opaque content — identical to a plain clear+draw.
        let mut comp = Compositor::new(300, 200, 60).unwrap();
        let id = comp.create_window("Solid".to_string(), 120, 80, 1);
        comp.move_window(id, 50, 40).unwrap();
        comp.submit_render(
            id,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
                color: Color::rgba(30, 34, 40, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();

        comp.bench_full_composite();

        // `bench_full_composite` swaps, so the composited result is in front.
        let bg = comp.theme.desktop_background;
        let front = comp.backend.presented_pixels();
        let stride = 300usize;
        let at = |x: usize, y: usize| front[y * stride + x];
        // A pixel well inside the client area carries the window content.
        assert_eq!(at(60, 50), 0xFF_1E_22_28);
        assert_ne!(at(60, 50), bg);
        // A pixel far from any window keeps the desktop background.
        assert_eq!(at(250, 150), bg);
        // A pixel just left of the client rect (still background region).
        assert_eq!(at(10, 50), bg);
    }

    // ---- text rendering ----------------------------------------------------

    const INK_W: u32 = 200;
    const INK_H: u32 = 120;
    /// Where [`paint`] puts the pen. Named because the overflow tests have to
    /// do arithmetic against it: `max_width` is measured from here, not from
    /// the left edge of the surface.
    const INK_X: i32 = 4;

    /// Draws `text` on a black surface and hands back the whole surface, so a
    /// test can compare two renderings pixel for pixel rather than only by
    /// where the ink landed.
    fn paint(
        text: &str,
        size: f32,
        weight: FontWeightHint,
        max_width: Option<u32>,
        overflow: TextOverflow,
    ) -> Framebuffer {
        let mut fb = Framebuffer::new(INK_W, INK_H).unwrap();
        fb.clear(0xFF_00_00_00);
        let mut engine = RenderEngine::new();
        engine.draw_text(
            &mut fb,
            INK_X,
            4,
            text,
            0xFF_FF_FF_FF,
            &[],
            1.0,
            max_width,
            size,
            weight,
            overflow,
        );
        fb
    }

    /// The rows and columns of `fb` that received ink, so a test can talk about
    /// where the glyphs landed instead of about individual pixels.
    fn ink_bounds(fb: &Framebuffer) -> (Vec<u32>, Vec<u32>) {
        let lit = |x: u32, y: u32| fb.get_pixel(x, y).is_some_and(|p| p & 0x00FF_FFFF != 0);
        let rows = (0..INK_H)
            .filter(|&y| (0..INK_W).any(|x| lit(x, y)))
            .collect();
        let cols = (0..INK_W)
            .filter(|&x| (0..INK_H).any(|y| lit(x, y)))
            .collect();
        (rows, cols)
    }

    /// `ink_bounds(paint(..))` under the policy that predates §427, which is
    /// what every test written before it assumed.
    fn ink_of(
        text: &str,
        size: f32,
        weight: FontWeightHint,
        max_width: Option<u32>,
    ) -> (Vec<u32>, Vec<u32>) {
        ink_bounds(&paint(text, size, weight, max_width, TextOverflow::Clip))
    }

    #[test]
    fn test_text_honors_font_size() {
        // The old private face was a fixed 8x14 cell, so `font_size` from an
        // app's render tree was parsed and then thrown away. Two sizes must now
        // produce visibly different text.
        let (small_rows, small_cols) = ink_of("Ag", 16.0, FontWeightHint::Regular, None);
        let (big_rows, big_cols) = ink_of("Ag", 48.0, FontWeightHint::Regular, None);
        assert!(!small_rows.is_empty(), "nothing drawn at 16px");
        assert!(
            big_rows.len() > small_rows.len(),
            "48px text covers {} rows, 16px covers {} — size was ignored",
            big_rows.len(),
            small_rows.len()
        );
        assert!(
            big_cols.len() > small_cols.len(),
            "48px text is no wider than 16px text"
        );
    }

    #[test]
    fn test_text_honors_bold_weight() {
        // Same reason: `font_weight` used to be discarded.
        let (_, regular) = ink_of("lll", 16.0, FontWeightHint::Regular, None);
        let (_, bold) = ink_of("lll", 16.0, FontWeightHint::Bold, None);
        assert!(
            bold.len() > regular.len(),
            "bold covers {} columns, regular {} — weight was ignored",
            bold.len(),
            regular.len()
        );
    }

    #[test]
    fn test_text_draws_beyond_ascii() {
        // The private face had glyphs for about ninety ASCII characters and
        // drew a filled box for everything else, which made any non-English
        // window title unreadable. Two different non-ASCII characters must now
        // produce two different shapes.
        let (rows_a, cols_a) = ink_of("\u{2500}", 16.0, FontWeightHint::Regular, None);
        let (rows_b, cols_b) = ink_of("\u{00e9}", 16.0, FontWeightHint::Regular, None);
        assert!(!rows_a.is_empty(), "box-drawing character drew nothing");
        assert!(!rows_b.is_empty(), "e-acute drew nothing");
        assert_ne!(
            (rows_a, cols_a),
            (rows_b, cols_b),
            "two different characters produced identical ink — both are tofu"
        );
    }

    #[test]
    fn test_text_max_width_drops_whole_glyphs() {
        // Truncation must happen between glyphs: half a letter reads as a
        // rendering fault rather than as elided text.
        let (_, unclipped) = ink_of("MMMMMMMM", 16.0, FontWeightHint::Regular, None);
        let (_, clipped) = ink_of("MMMMMMMM", 16.0, FontWeightHint::Regular, Some(20));
        assert!(!clipped.is_empty(), "max_width dropped everything");
        assert!(
            clipped.len() < unclipped.len(),
            "max_width did not truncate"
        );
        let last = clipped.iter().max().copied().unwrap();
        assert!(
            last < 4 + 20,
            "ink at column {last} spills past the 20px limit from x=4"
        );
    }

    // ---- text overflow (design-decisions.md §427) --------------------------
    //
    // These tests are deliberately written against *metric-independent*
    // properties — "the two renderings differ", "the ink stays inside the
    // limit" — rather than against pixel coordinates derived from a guess at
    // how wide `M` or `…` is in the system face. A test that encodes guessed
    // font metrics fails the first time the face is revised, which teaches the
    // next reader to delete it rather than to trust it.

    /// True when two renderings are identical pixel for pixel.
    fn same_pixels(a: &Framebuffer, b: &Framebuffer) -> bool {
        (0..INK_H).all(|y| (0..INK_W).all(|x| a.get_pixel(x, y) == b.get_pixel(x, y)))
    }

    /// A string long enough to overrun any limit these tests set.
    const LONG: &str = "MMMMMMMMMMMMMMMM";

    #[test]
    fn test_text_ellipsis_marks_a_cut_that_clip_leaves_silent() {
        // The whole of §427: with `Clip`, a truncated label is indistinguishable
        // from a complete one. With `Ellipsis` it is not.
        let clip = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            Some(60),
            TextOverflow::Clip,
        );
        let ell = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            Some(60),
            TextOverflow::Ellipsis,
        );
        assert!(
            !same_pixels(&clip, &ell),
            "an overrunning run rendered identically under both policies — \
             the overflow field reached the compositor and was ignored"
        );
        let (_, cols) = ink_bounds(&ell);
        assert!(!cols.is_empty(), "the ellipsis policy drew nothing at all");
    }

    #[test]
    fn test_text_ellipsis_is_not_earned_by_a_string_that_fits() {
        // A mark on a label that was never cut is a lie in the other direction:
        // it tells the reader there is more to the value than there is.
        for max_width in [Some(180), Some(u32::MAX)] {
            let clip = paint(
                "M",
                16.0,
                FontWeightHint::Regular,
                max_width,
                TextOverflow::Clip,
            );
            let ell = paint(
                "M",
                16.0,
                FontWeightHint::Regular,
                max_width,
                TextOverflow::Ellipsis,
            );
            assert!(
                same_pixels(&clip, &ell),
                "a string that fits inside {max_width:?} was decorated with a mark"
            );
        }
    }

    #[test]
    fn test_text_ellipsis_without_a_bound_is_vacuous() {
        // `max_width: None` cannot overflow, so the policy has nothing to say.
        // Every unbounded site in the tree was swept to `Clip` on exactly this
        // reasoning, so it had better be true.
        let clip = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            None,
            TextOverflow::Clip,
        );
        let ell = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            None,
            TextOverflow::Ellipsis,
        );
        assert!(
            same_pixels(&clip, &ell),
            "the overflow policy changed an unbounded rendering"
        );
    }

    #[test]
    fn test_text_ellipsis_stays_inside_max_width() {
        // The mark has to fit inside the bound too, or it is just a
        // differently-shaped overflow — the failure it exists to report.
        for w in [24u32, 40, 60, 90, 120] {
            let (_, cols) = ink_bounds(&paint(
                LONG,
                16.0,
                FontWeightHint::Regular,
                Some(w),
                TextOverflow::Ellipsis,
            ));
            if let Some(&last) = cols.iter().max() {
                let limit = INK_X as u32 + w;
                assert!(
                    last < limit,
                    "at max_width {w} the mark put ink at column {last}, \
                     past the limit at {limit}"
                );
            }
        }
    }

    #[test]
    fn test_text_ellipsis_falls_back_to_clipping_when_the_mark_does_not_fit() {
        // Derived, not guessed: a glyph's ink is never wider than its advance,
        // so a bound below the mark's *ink* width is certainly below its
        // advance. That makes this the one width the test can be sure of
        // without knowing the face's metrics.
        let (_, mark_cols) = ink_bounds(&paint(
            "…",
            16.0,
            FontWeightHint::Regular,
            None,
            TextOverflow::Clip,
        ));
        let mark_ink = mark_cols.iter().max().copied().unwrap() + 1 - INK_X as u32;
        assert!(
            mark_ink > 2,
            "the ellipsis glyph rendered as {mark_ink}px of ink"
        );
        let w = mark_ink - 2;

        let clip = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            Some(w),
            TextOverflow::Clip,
        );
        let ell = paint(
            LONG,
            16.0,
            FontWeightHint::Regular,
            Some(w),
            TextOverflow::Ellipsis,
        );
        assert!(
            same_pixels(&clip, &ell),
            "at max_width {w}, too narrow for the mark itself, the ellipsis \
             policy drew something other than a plain clip"
        );
    }

    #[test]
    fn test_text_ellipsis_never_blanks_a_field_that_clipping_would_fill() {
        // Reserving room for the mark shortens the run, so the arithmetic could
        // in principle leave nothing at all — a field that reads as empty rather
        // than as truncated, which is worse than what §427 set out to fix.
        for w in [16u32, 24, 40, 60, 90, 120, 199] {
            let (_, clip) = ink_bounds(&paint(
                LONG,
                16.0,
                FontWeightHint::Regular,
                Some(w),
                TextOverflow::Clip,
            ));
            if clip.is_empty() {
                continue;
            }
            let (_, ell) = ink_bounds(&paint(
                LONG,
                16.0,
                FontWeightHint::Regular,
                Some(w),
                TextOverflow::Ellipsis,
            ));
            assert!(
                !ell.is_empty(),
                "at max_width {w} clipping drew {} columns of ink and the \
                 ellipsis policy drew none",
                clip.len()
            );
        }
    }

    #[test]
    fn test_text_at_absurd_coordinates_is_clipped_not_wrapped() {
        // Coordinates arrive from another process, so they may be anything.
        // A wrapped or NaN-folded coordinate would land back inside the
        // framebuffer and paint over unrelated pixels.
        let mut fb = Framebuffer::new(64, 32).unwrap();
        fb.clear(0xFF_00_00_00);
        let mut engine = RenderEngine::new();
        for (x, y) in [
            (i32::MIN, 8),
            (i32::MAX, 8),
            (8, i32::MIN),
            (8, i32::MAX),
            (-1000, -1000),
        ] {
            // Both policies, and an extreme `max_width` with each: the ellipsis
            // path does its own arithmetic on `x` and the limit, so an absurd
            // coordinate reaches code the `Clip` path never runs.
            for overflow in [TextOverflow::Clip, TextOverflow::Ellipsis] {
                for max_width in [None, Some(0), Some(1), Some(u32::MAX)] {
                    engine.draw_text(
                        &mut fb,
                        x,
                        y,
                        "leak",
                        0xFF_FF_FF_FF,
                        &[],
                        1.0,
                        max_width,
                        16.0,
                        FontWeightHint::Regular,
                        overflow,
                    );
                }
            }
        }
        for y in 0..32 {
            for x in 0..64 {
                assert_eq!(
                    fb.get_pixel(x, y),
                    Some(0xFF_00_00_00),
                    "off-surface text leaked into ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn test_light_weight_falls_back_to_regular_not_bold() {
        // The built-in face has two weights. Rendering `Light` as bold would
        // be the opposite of what was asked for, so it must map to regular.
        assert_eq!(weight_of(FontWeightHint::Light), Weight::Regular);
        assert_eq!(weight_of(FontWeightHint::Regular), Weight::Regular);
        assert_eq!(weight_of(FontWeightHint::Bold), Weight::Bold);
    }

    #[test]
    fn test_font_size_from_a_hostile_client_still_draws() {
        // `font_size` crosses a process boundary, so it is not necessarily a
        // number at all. A nonsense size must fall back to a readable one
        // rather than panicking or drawing nothing.
        for size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            let (rows, _) = ink_of("W", size, FontWeightHint::Regular, None);
            assert!(!rows.is_empty(), "font_size = {size} drew nothing");
        }
        // An enormous size is clamped rather than honoured — a 1e30-pixel face
        // would be a memory bomb — so its glyphs are merely too big for this
        // 200x120 surface. What matters is that asking does not panic.
        let (rows, _) = ink_of("W", 1e30, FontWeightHint::Regular, None);
        assert!(rows.is_empty(), "a 512px glyph should not fit in 120 rows");
    }

    // ---- rich text (per-glyph colour) --------------------------------------

    /// [`paint`] with a span list, so a test can compare a multi-coloured
    /// rendering against the plain one of the same string.
    fn paint_rich(text: &str, size: f32, spans: &[TextSpan]) -> Framebuffer {
        let mut fb = Framebuffer::new(INK_W, INK_H).unwrap();
        fb.clear(0xFF_00_00_00);
        let mut engine = RenderEngine::new();
        engine.draw_text(
            &mut fb,
            INK_X,
            4,
            text,
            0xFF_FF_FF_FF,
            spans,
            1.0,
            None,
            size,
            FontWeightHint::Regular,
            TextOverflow::Clip,
        );
        fb
    }

    /// Every pixel that received ink, as (x, y, rgb).
    fn lit_pixels(fb: &Framebuffer) -> Vec<(u32, u32, u32)> {
        let mut out = Vec::new();
        for y in 0..INK_H {
            for x in 0..INK_W {
                if let Some(p) = fb.get_pixel(x, y)
                    && p & 0x00FF_FFFF != 0
                {
                    out.push((x, y, p & 0x00FF_FFFF));
                }
            }
        }
        out
    }

    /// **The regression that matters.** Colouring by span must change colours
    /// and nothing else: the glyphs must land in exactly the positions the plain
    /// command puts them in. The whole reason this primitive exists is that
    /// cutting the string per colour *moved* the glyphs; a rich-text path that
    /// laid out even slightly differently would have reintroduced the bug in a
    /// place no colour assertion looks.
    #[test]
    fn rich_text_lays_out_exactly_as_plain_text_does() {
        let sample = "let x = fn(1, 2);";
        let plain = paint(
            sample,
            20.0,
            FontWeightHint::Regular,
            None,
            TextOverflow::Clip,
        );
        let rich = paint_rich(
            sample,
            20.0,
            &[
                TextSpan {
                    end: 3,
                    color: Color::rgb(255, 0, 0),
                },
                TextSpan {
                    end: 5,
                    color: Color::rgb(0, 255, 0),
                },
                TextSpan {
                    end: 17,
                    color: Color::rgb(0, 0, 255),
                },
            ],
        );
        let plain_at: Vec<(u32, u32)> =
            lit_pixels(&plain).iter().map(|&(x, y, _)| (x, y)).collect();
        let rich_at: Vec<(u32, u32)> = lit_pixels(&rich).iter().map(|&(x, y, _)| (x, y)).collect();
        assert!(!plain_at.is_empty(), "nothing drawn");
        assert_eq!(
            plain_at, rich_at,
            "rich text put ink in different pixels from plain text",
        );
    }

    /// A glyph takes the colour of the byte it came from, so the string's two
    /// halves come out in their two colours — and, since the primitive shapes
    /// once, the boundary falls between glyphs rather than at a guessed x.
    #[test]
    fn each_glyph_takes_the_colour_of_its_span() {
        // Two characters, two colours, and channels chosen so a pixel can be
        // attributed to one span or the other however the antialiasing scales
        // it: red-only ink can only have come from the first span.
        let fb = paint_rich(
            "HI",
            40.0,
            &[
                TextSpan {
                    end: 1,
                    color: Color::rgb(255, 0, 0),
                },
                TextSpan {
                    end: 2,
                    color: Color::rgb(0, 0, 255),
                },
            ],
        );
        let lit = lit_pixels(&fb);
        assert!(!lit.is_empty(), "nothing drawn");
        let reds: Vec<u32> = lit
            .iter()
            .filter(|&&(_, _, c)| c & 0x00_FF_00 == 0 && c & 0x00_00_FF == 0)
            .map(|&(x, _, _)| x)
            .collect();
        let blues: Vec<u32> = lit
            .iter()
            .filter(|&&(_, _, c)| c & 0xFF_00_00 == 0 && c & 0x00_FF_00 == 0)
            .map(|&(x, _, _)| x)
            .collect();
        assert!(
            !reds.is_empty(),
            "no red ink — the first span was not applied"
        );
        assert!(
            !blues.is_empty(),
            "no blue ink — the second span was not applied"
        );
        assert!(
            lit.len() == reds.len() + blues.len(),
            "{} pixels are neither span's colour",
            lit.len() - reds.len() - blues.len(),
        );
        // In left-to-right text the first byte's glyph is also the leftmost, so
        // the two colours must not interleave in x.
        let (red_max, blue_min) = (
            reds.iter().copied().max().unwrap(),
            blues.iter().copied().min().unwrap(),
        );
        assert!(
            red_max < blue_min,
            "colours interleave: red runs to x={red_max}, blue starts at x={blue_min}",
        );
    }

    /// An empty span list has to be indistinguishable from a plain `Text`
    /// command — same glyphs, same colour — because that is the fallback every
    /// caller with nothing to say lands on.
    #[test]
    fn rich_text_with_no_spans_is_plain_text() {
        let sample = "fallback";
        let plain = paint(
            sample,
            24.0,
            FontWeightHint::Regular,
            None,
            TextOverflow::Clip,
        );
        let rich = paint_rich(sample, 24.0, &[]);
        assert_eq!(lit_pixels(&plain), lit_pixels(&rich));
    }

    /// Bytes past the last span fall through to the command's own colour rather
    /// than borrowing the last span's. A tokenizer that stops short of the end
    /// of a line is the ordinary way to reach this.
    #[test]
    fn bytes_past_the_last_span_take_the_base_colour() {
        // `paint_rich` draws in white, so the tail must be white while the
        // covered head is red.
        let fb = paint_rich(
            "HI",
            40.0,
            &[TextSpan {
                end: 1,
                color: Color::rgb(255, 0, 0),
            }],
        );
        let lit = lit_pixels(&fb);
        assert!(!lit.is_empty(), "nothing drawn");
        // White ink has all three channels lit; red-only ink has one.
        let white = lit
            .iter()
            .filter(|&&(_, _, c)| c & 0x00_FF_00 != 0 && c & 0x00_00_FF != 0)
            .count();
        let red = lit
            .iter()
            .filter(|&&(_, _, c)| c & 0x00_FF_00 == 0 && c & 0x00_00_FF == 0)
            .count();
        assert!(red > 0, "the covered byte was not coloured by its span");
        assert!(
            white > 0,
            "the uncovered byte did not fall back to the base colour"
        );
    }

    /// The overflow mark stands for text that was *not* drawn, so it belongs to
    /// no byte and takes the base colour — not the colour of byte zero, which is
    /// what passing the spans to the mark's own run would give it.
    #[test]
    fn the_overflow_mark_is_not_coloured_by_the_spans() {
        let mut fb = Framebuffer::new(INK_W, INK_H).unwrap();
        fb.clear(0xFF_00_00_00);
        let mut engine = RenderEngine::new();
        let long = "W".repeat(60);
        engine.draw_text(
            &mut fb,
            INK_X,
            4,
            &long,
            0xFF_FF_FF_FF,
            // One span over the whole string, in a colour with no white in it.
            &[TextSpan {
                end: 60,
                color: Color::rgb(255, 0, 0),
            }],
            1.0,
            Some(120),
            20.0,
            FontWeightHint::Regular,
            TextOverflow::Ellipsis,
        );
        let lit = lit_pixels(&fb);
        assert!(!lit.is_empty(), "nothing drawn");
        let white = lit
            .iter()
            .filter(|&&(_, _, c)| c & 0x00_FF_00 != 0 && c & 0x00_00_FF != 0)
            .count();
        assert!(
            white > 0,
            "the ellipsis was drawn in the span's colour rather than the base colour",
        );
    }

    // ---------------------------------------------------------------------
    // Stacking layers
    //
    // The defect these exist for: before `Layer`, `z_stack` was one flat list
    // and every raise went to the very top of it. A taskbar was therefore an
    // ordinary window that the next click on any application put behind that
    // application — which is not a cosmetic ordering complaint, it is a
    // taskbar that vanishes the moment the desktop is used.
    // ---------------------------------------------------------------------

    /// Create a window in a named band, since `create_window` cannot say one.
    fn layered(comp: &mut Compositor, title: &str, layer: Layer) -> WindowId {
        let mut spec = WindowSpec::new(title, 200, 100);
        spec.layer = layer;
        comp.create_window_from_spec(&spec, 1)
    }

    #[test]
    fn an_overlay_stays_above_an_application_however_often_it_is_raised() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let panel = layered(&mut comp, "Taskbar", Layer::Overlay);
        let app = layered(&mut comp, "Editor", Layer::Normal);

        // Creating the application after the panel must not have put it on top.
        assert_eq!(
            comp.z_stack.last(),
            Some(&panel),
            "a new application window was stacked over the taskbar"
        );

        // Nor may raising it, which is the operation that actually happens
        // every time a user clicks a window.
        comp.focus_window(app);
        assert_eq!(
            comp.z_stack.last(),
            Some(&panel),
            "clicking an application window put it over the taskbar"
        );
        assert_eq!(
            comp.focused_window,
            Some(app),
            "the application should still have taken focus; only the stacking              is confined, not the focus"
        );
    }

    #[test]
    fn a_background_surface_cannot_climb_over_an_application() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let wallpaper = layered(&mut comp, "Wallpaper", Layer::Background);
        let app = layered(&mut comp, "Editor", Layer::Normal);

        comp.focus_window(wallpaper);
        assert_eq!(
            comp.z_stack,
            vec![wallpaper, app],
            "raising the wallpaper lifted it over the window it is behind"
        );
    }

    #[test]
    fn raising_reorders_within_a_band_exactly_as_it_always_did() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let a = layered(&mut comp, "A", Layer::Normal);
        let b = layered(&mut comp, "B", Layer::Normal);
        let c = layered(&mut comp, "C", Layer::Normal);
        assert_eq!(comp.z_stack, vec![a, b, c]);

        comp.focus_window(a);
        assert_eq!(
            comp.z_stack,
            vec![b, c, a],
            "confining a raise to its band must not change what a raise does              inside the band"
        );
    }

    #[test]
    fn the_stack_stays_partitioned_by_band_under_arbitrary_raises() {
        // `stack_insertion_index` counts rather than searches, which is only
        // correct while the stack is partitioned. This walks every window in
        // turn, repeatedly, and re-checks the invariant the counting rests on.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let ids: Vec<WindowId> = [
            Layer::Normal,
            Layer::Overlay,
            Layer::Background,
            Layer::Normal,
            Layer::Overlay,
            Layer::Background,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, l)| layered(&mut comp, &format!("w{i}"), l))
        .collect();

        for round in 0..3 {
            for &id in &ids {
                comp.focus_window(id);
                let layers: Vec<Layer> = comp.z_stack.iter().map(|&i| comp.layer_of(i)).collect();
                assert!(
                    layers.windows(2).all(|w| w[0] <= w[1]),
                    "round {round}: raising {id:?} left the stack unsorted by                      band: {layers:?}"
                );
                assert_eq!(
                    comp.z_stack.len(),
                    ids.len(),
                    "a raise lost or duplicated a window"
                );
            }
        }
    }

    #[test]
    fn closing_an_application_does_not_hand_focus_to_the_taskbar() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let panel = layered(&mut comp, "Taskbar", Layer::Overlay);
        let behind = layered(&mut comp, "Behind", Layer::Normal);
        let front = layered(&mut comp, "Front", Layer::Normal);
        comp.focus_window(front);

        comp.destroy_window(front).unwrap();
        assert_eq!(
            comp.focused_window,
            Some(behind),
            "closing a window focused the topmost surface outright, which is              always the shell"
        );
        assert!(comp.window_ref(panel).is_some(), "the panel was destroyed");
    }

    #[test]
    fn closing_the_last_application_leaves_the_taskbar_unfocused() {
        // The fallback has to be "nothing" rather than "whatever is left":
        // with no application open the user is looking at the desktop, not at
        // the taskbar's keyboard focus.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let _panel = layered(&mut comp, "Taskbar", Layer::Overlay);
        let only = layered(&mut comp, "Only", Layer::Normal);
        comp.focus_window(only);

        comp.destroy_window(only).unwrap();
        assert_eq!(comp.focused_window, None);
    }

    #[test]
    fn a_window_that_never_names_a_layer_is_an_ordinary_window() {
        // Every caller that predates `Layer` goes through here, so this is the
        // test that the change is invisible to them.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Legacy".to_string(), 300, 200, 1);
        assert_eq!(comp.window_ref(id).unwrap().layer, Layer::Normal);
    }

    // -----------------------------------------------------------------------
    // Display scaling of the decorations
    //
    // `Display::scale_factor` existed from the start and its only reader was
    // the wire report that tells a client what its display's scale is. The
    // compositor drew every frame at 96dpi regardless, so on a 2× display the
    // title bar was half the height it should be and the buttons a quarter of
    // the area — small enough to be hard to hit, which is the practical
    // symptom. These tests pin the whole chain: the scale is picked up, it
    // reaches geometry, hit testing agrees with drawing, and nothing rounds to
    // an unusable zero.
    // -----------------------------------------------------------------------

    /// A compositor whose single display has the given scale, plus a second
    /// display of the given scale immediately to its right.
    ///
    /// Both are 800×600, so the seam is at x = 800 and a window's share of
    /// each side is easy to state.
    fn two_displays(left_scale: f32, right_scale: f32) -> Compositor {
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        if let Some(d) = comp.display_manager.displays.first_mut() {
            d.scale_factor = left_scale;
        }
        comp.attach_display(Display::new(1, 800, 600, 60, right_scale, false))
            .expect("attach");
        comp
    }

    #[test]
    fn a_window_on_a_2x_display_gets_a_2x_title_bar_and_2x_buttons() {
        let mut comp = two_displays(2.0, 1.0);
        let mut spec = WindowSpec::new("HiDPI", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);

        comp.refresh_window_scales();

        let win = comp.window_ref(id).expect("window");
        assert!(
            (win.scale_factor - 2.0).abs() < f32::EPSILON,
            "the window never learned its display's scale: {}",
            win.scale_factor
        );

        let (top, side, _) = win.frame_insets();
        assert_eq!(
            top,
            TITLE_BAR_HEIGHT * 2,
            "the title bar is still the 96dpi height"
        );
        assert_eq!(side, BORDER_WIDTH * 2);
        assert_eq!(win.shadow_extent(), SHADOW_SIZE * 2);

        let close = win.close_button_rect().expect("a decorated window closes");
        assert_eq!(
            (close.width, close.height),
            (TITLE_BUTTON_SIZE * 2, TITLE_BUTTON_SIZE * 2),
            "the buttons did not grow with the bar, so they are a quarter of \
             the area the user expects to hit"
        );

        // The client area is emphatically *not* scaled: it is the client's own
        // pixels, and resizing a window behind its owner's back is a different
        // and much worse bug than a small title bar.
        assert_eq!(win.client_rect(), Rect::new(100, 100, 200, 150));
    }

    #[test]
    fn a_scaled_title_bar_is_still_grabbable_where_it_is_drawn() {
        // The point of applying the scale inside `frame_insets` rather than at
        // each drawing site: a title bar drawn at 2× and hit-tested at 1× is a
        // title bar the user can see but cannot drag. The dead band is the
        // lower half of the bar — exactly where a user aims.
        let mut comp = two_displays(2.0, 1.0);
        let mut spec = WindowSpec::new("Grab", 200, 150);
        spec.position = Some((100, 200));
        let id = comp.create_window_from_spec(&spec, 1);

        // Deliberately *not* refreshed by hand: the whole point is that input
        // routing brings the scale up to date itself. A drag delivers pointer
        // motion far faster than frames are composed, so relying on
        // `compose_frame` to have done it would leave the grab slipping out
        // from under the pointer for the rest of the frame.
        //
        // y = 155 with the client area starting at 200: 45 px above the client
        // edge. The 2× bar spans 140..200 and contains it; the 1× bar spans
        // 170..200 and does not, so an unscaled hit test puts this press on the
        // desktop and starts no drag at all.
        let (px, py) = (200, 155);
        comp.handle_input(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
            x: px,
            y: py,
        });

        let bar = comp
            .window_ref(id)
            .and_then(Window::title_bar_rect)
            .expect("framed");
        assert_eq!(bar.height, TITLE_BAR_HEIGHT * 2);
        assert!(bar.contains(px, py), "the press was meant to be on the bar");

        let drag = comp
            .drag
            .clone()
            .expect("pressing the title bar starts a move");
        assert_eq!(drag.mode, DragMode::MoveWindow);
        assert_eq!(drag.window_id, id);

        // And it really moves, rather than merely arming.
        comp.handle_input(InputEvent::MouseMove {
            x: px + 40,
            y: py + 25,
        });
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.x, win.y), (140, 225));
    }

    #[test]
    fn a_scaled_shadow_is_actually_drawn_to_its_scaled_extent() {
        // `shadow_extent` is the layer count as well as the geometry, and
        // `render_shadow` used to loop over the raw `SHADOW_SIZE`. That leaves
        // a 2× window with an 8-layer shadow inside a 16 px allowance: the
        // damage test above still passes (under-drawing is inside the extent),
        // and the visible result is a shadow that stops halfway with a hard
        // edge. So this asserts on the pixel that only the outer half reaches.
        let mut comp = Compositor::new(400, 300, 60).expect("compositor");
        if let Some(d) = comp.display_manager.displays.first_mut() {
            d.scale_factor = 2.0;
        }
        let mut spec = WindowSpec::new("Framed", 120, 90);
        spec.position = Some((160, 140));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();

        let bg = comp.theme.desktop_background;
        comp.backend.clear(bg);
        comp.render_window(id);

        let win = comp.window_ref(id).expect("window");
        let (frame, extent) = (win.frame_rect(), win.shadow_extent());
        assert_eq!(extent, SHADOW_SIZE * 2);

        // How far right of the frame box anything is painted, on a row level
        // with the middle of the window. Measured rather than predicted from a
        // formula: re-deriving `render_shadow`'s arithmetic here would just be
        // a second copy of it that can agree with a wrong original.
        #[allow(
            clippy::cast_sign_loss,
            reason = "the sampled row and columns are inside the 400x300 buffer"
        )]
        let row = (frame.y + frame.height as i32 / 2) as u32;
        let rightmost = (0..400u32)
            .rev()
            .find(|&x| working_pixel(&comp.backend, x, row) != Some(bg))
            .expect("the window is on screen");
        let reach = rightmost as i32 + 1 - frame.right();

        // The invariant, which holds at any scale: the shadow reaches at least
        // as far beyond the frame as the extent that was reserved for it. At 1×
        // the reach is 10 against an extent of 8. A 2× window whose shadow is
        // still drawn in 8 layers reaches 13 against an extent of 16, and looks
        // like a shadow cut off with a knife.
        assert!(
            reach >= extent as i32,
            "the shadow reaches {reach}px past the frame but {extent}px were \
             reserved for it: it stops where a 1x shadow would"
        );
    }

    #[test]
    fn a_window_dragged_mostly_onto_the_second_monitor_adopts_its_scale() {
        // Largest intersection, not "the display containing the top-left
        // corner". Under the corner rule this window keeps the left monitor's
        // 1× decorations while three quarters of it sits on the 2× monitor.
        let mut comp = two_displays(1.0, 2.0);
        let mut spec = WindowSpec::new("Straddler", 200, 150);
        spec.position = Some((0, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();
        assert!(
            (comp.window_ref(id).unwrap().scale_factor - 1.0).abs() < f32::EPSILON,
            "it started on the left monitor"
        );

        // 50 px of the 200 px width remain left of the seam at x = 800.
        comp.move_window(id, 750, 100).expect("move");
        comp.refresh_window_scales();
        let win = comp.window_ref(id).expect("window");
        assert!(
            (win.scale_factor - 2.0).abs() < f32::EPSILON,
            "three quarters of it is on the 2x monitor and it is still drawn \
             at {}x",
            win.scale_factor
        );

        // The other way round, to prove it is a rule and not a one-way latch.
        // 650 rather than 700: at 700 the window straddles the seam in exact
        // halves, and an exact tie deliberately goes to the rightmost display
        // (`max_by_key` returns the last maximum). Testing the tie here would
        // be testing the tiebreak, not the rule.
        comp.move_window(id, 650, 100).expect("move");
        comp.refresh_window_scales();
        assert!(
            (comp.window_ref(id).unwrap().scale_factor - 1.0).abs() < f32::EPSILON,
            "three quarters of it went back and it kept the second monitor's \
             scale"
        );

        // And the tie itself, pinned so that changing it is a deliberate act:
        // 100 px on each side.
        comp.move_window(id, 700, 100).expect("move");
        comp.refresh_window_scales();
        assert!(
            (comp.window_ref(id).unwrap().scale_factor - 2.0).abs() < f32::EPSILON,
            "a window split exactly across the seam should take the rightmost \
             display's scale, deterministically rather than by hotplug order"
        );
    }

    #[test]
    fn a_window_dragged_off_every_display_keeps_a_usable_scale() {
        // Falling back to the primary rather than to nothing: a window in the
        // gap beyond the right-hand monitor still has to be drawn, and there is
        // no such thing as "no scale".
        let mut comp = two_displays(1.0, 2.0);
        let mut spec = WindowSpec::new("Adrift", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);

        comp.window_mut(id).expect("window").x = 100_000;
        comp.refresh_window_scales();
        let win = comp.window_ref(id).expect("window");
        assert!(
            (win.scale_factor - 1.0).abs() < f32::EPSILON,
            "off-screen windows are drawn at the primary display's scale"
        );
        assert_eq!(win.frame_insets().0, TITLE_BAR_HEIGHT);
    }

    #[test]
    fn a_border_never_scales_down_to_nothing() {
        // `BORDER_WIDTH` is 1, so any scale below 1.5 rounds it to 1 or to 0 —
        // and a 0 px border is not a thin border, it is a window whose edge
        // cannot be grabbed to resize it, because `detect_border_drag` reads
        // the same inset. Sub-1× scales are real: a 4K panel driven at a
        // fractional scale, or a projector configured down.
        assert_eq!(scale_dimension(BORDER_WIDTH, 0.5), 1);
        assert_eq!(scale_dimension(BORDER_WIDTH, 0.01), 1);
        assert_eq!(scale_dimension(SHADOW_SIZE, 0.05), 1);
        // A dimension that was already nothing stays nothing: this is the
        // unframed case, and inventing a 1 px title bar for a tooltip would be
        // worse than the bug being guarded against.
        assert_eq!(scale_dimension(0, 4.0), 0);
        // A nonsense scale cannot produce a nonsense size. The cast saturates
        // at 0 for negatives and NaN, which is why the clamp is applied to the
        // float before the cast rather than to the integer after it.
        assert_eq!(scale_dimension(TITLE_BAR_HEIGHT, -3.0), 1);
        assert_eq!(scale_dimension(TITLE_BAR_HEIGHT, f32::NAN), 1);

        let mut comp = two_displays(0.5, 1.0);
        let mut spec = WindowSpec::new("Downscaled", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();
        let win = comp.window_ref(id).expect("window").clone();
        assert_eq!(win.frame_insets().1, 1, "the resize edge vanished");
        assert!(
            comp.detect_border_drag(&win, win.x - 1, win.y + 10)
                .is_some(),
            "a 0.5x window cannot be resized by its left edge"
        );
    }

    #[test]
    fn nothing_a_scaled_window_draws_falls_outside_its_damage_extent() {
        // The 1× version of this test is
        // `nothing_a_window_draws_falls_outside_its_damage_extent`. It passed
        // throughout the period when `window_drawn_extent` inflated by the raw
        // `SHADOW_SIZE`, because at 1× the raw constant *is* the scaled value.
        // A 2× window casts a 16 px shadow into an 8 px allowance, and the
        // symptom — a drop shadow that ends in a hard edge — looks nothing like
        // its cause.
        let mut comp = Compositor::new(400, 300, 60).expect("compositor");
        if let Some(d) = comp.display_manager.displays.first_mut() {
            d.scale_factor = 2.0;
        }
        let mut spec = WindowSpec::new("Framed", 120, 90);
        spec.position = Some((160, 140));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();
        assert_eq!(comp.window_ref(id).expect("window").shadow_extent(), 16);

        let bg = comp.theme.desktop_background;
        comp.backend.clear(bg);
        comp.render_window(id);

        let extent = comp
            .window_ref(id)
            .map(Compositor::window_drawn_extent)
            .expect("window");
        for y in 0..300u32 {
            for x in 0..400u32 {
                if working_pixel(&comp.backend, x, y) == Some(bg) {
                    continue;
                }
                assert!(
                    extent.contains(x as i32, y as i32),
                    "painted ({x}, {y}), outside the damage extent {extent:?}"
                );
            }
        }
    }

    #[test]
    fn a_window_that_shrinks_its_frame_repaints_the_box_it_vacated() {
        // Going 2× → 1× is the direction that matters. Growing, the new box
        // contains the old one and damaging only the new box happens to be
        // enough; shrinking, the old box is the larger, and damaging only the
        // new one leaves the outside of the old decorations on screen with
        // nothing that will ever clean it up.
        let mut comp = two_displays(1.0, 2.0);
        let mut spec = WindowSpec::new("Shrinker", 200, 150);
        spec.position = Some((900, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();
        assert!(
            (comp.window_ref(id).expect("window").scale_factor - 2.0).abs() < f32::EPSILON,
            "it started on the 2x monitor"
        );

        comp.move_window(id, 100, 100).expect("move");
        // Cleared *after* the move: `move_window` damages the old and new
        // positions itself, and leaving that in would let this test pass on
        // damage that has nothing to do with the scale. What remains is the
        // window sitting still at its new home, still wearing 2× decorations.
        comp.damage.clear();
        let before = comp.window_ref(id).expect("window").outer_rect();

        comp.refresh_window_scales();
        let after = comp.window_ref(id).expect("window").outer_rect();
        assert!(
            after.width < before.width,
            "the frame did not shrink: {before:?} -> {after:?}"
        );
        assert!(
            comp.window_ref(id).expect("window").dirty,
            "a window whose frame changed size was left clean"
        );

        // The band the shadow vacated: just outside the new box, well inside
        // the old one. Nothing else in this test will ever repaint it.
        let vacated = (after.right() + 1, after.y + 20);
        assert!(before.contains(vacated.0, vacated.1));
        let damaged = comp.damage.rects().to_vec();
        assert!(
            damaged.iter().any(|r| r.contains(vacated.0, vacated.1)),
            "the vacated pixel {vacated:?} was never damaged; damage was \
             {damaged:?}"
        );
    }

    #[test]
    fn composing_a_frame_notices_a_window_that_moved_to_another_display() {
        // The refresh has to run *before* `compose_frame`'s damage check, not
        // after: the growth of the frame is itself the damage, so asking "is
        // there anything to draw?" first would answer no and leave the window
        // at the old display's size until something unrelated dirtied it.
        let mut comp = two_displays(1.0, 2.0);
        let mut spec = WindowSpec::new("Mover", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        assert!(comp.compose_frame(), "the first frame draws everything");
        comp.damage.clear();
        comp.full_recomposite = false;
        // The vsync gate would otherwise refuse a second frame this soon, and
        // this test is about the damage check, not about frame pacing.
        comp.frame_stats.last_frame_start = None;

        // Moved by hand rather than through `move_window`, so that the *only*
        // thing that can produce damage is the scale refresh itself.
        comp.window_mut(id).expect("window").x = 900;
        assert!(!comp.damage.has_damage(), "nothing has asked for a repaint");

        assert!(
            comp.compose_frame(),
            "the frame changed size and no frame was composited"
        );
        assert_eq!(
            comp.window_ref(id).expect("window").frame_insets().0,
            TITLE_BAR_HEIGHT * 2
        );
    }

    // ---------------------------------------------------------------------
    // Rounded rectangles
    //
    // The defect these exist for: `execute_command` destructured
    // `corner_radii: _` on `FillRect`, `StrokeRect` and `BoxShadow`. Every
    // rounded corner the toolkit, the SVG renderer, the tab strip, the
    // launcher and the shell had ever asked for was rasterized square, and
    // nothing anywhere compared the request against the result — the radii
    // were produced, serialized faithfully over the wire, and then dropped
    // by the one piece of code that could act on them.
    // ---------------------------------------------------------------------

    const ROUND_SIDE: u32 = 64;
    const ROUND_BG: u32 = 0xFF_00_00_00;
    const ROUND_FG: u32 = 0xFF_FF_FF_FF;
    /// The box every test below draws, inset from the canvas so that "nothing
    /// spilled" has somewhere to be observed.
    const BOX_X: i32 = 8;
    const BOX_Y: i32 = 8;
    const BOX_SIDE: u32 = 48;

    fn round_canvas() -> (RenderEngine, Framebuffer) {
        let mut fb = Framebuffer::new(ROUND_SIDE, ROUND_SIDE).expect("framebuffer");
        fb.clear(ROUND_BG);
        (RenderEngine::new(), fb)
    }

    /// How much ink a pixel got, `0..=255`.
    ///
    /// White on black, so any one colour channel *is* the coverage the
    /// rasterizer decided on — which is what lets these tests check
    /// antialiasing at all rather than only presence or absence.
    fn ink(fb: &Framebuffer, x: u32, y: u32) -> u32 {
        fb.get_pixel(x, y).unwrap_or(0) & 0xFF
    }

    /// Every pixel with any ink in it.
    fn inked(fb: &Framebuffer) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for y in 0..ROUND_SIDE {
            for x in 0..ROUND_SIDE {
                if ink(fb, x, y) > 0 {
                    out.push((x, y));
                }
            }
        }
        out
    }

    /// A radius that cannot move a pixel must not cost one either: these two
    /// inputs have to produce the *framebuffer* the flat path produces, not
    /// merely something that looks similar. The fall-through in
    /// `RoundRect::new` is what every existing decoration test depends on
    /// still being true.
    #[test]
    fn a_radius_too_small_to_see_draws_exactly_what_the_flat_path_draws() {
        for radius in [0.0_f32, 0.49] {
            let (engine, mut flat) = round_canvas();
            engine.fill_rect(&mut flat, BOX_X, BOX_Y, BOX_SIDE, BOX_SIDE, ROUND_FG, 1.0);

            let (engine, mut rounded) = round_canvas();
            engine.fill_round_rect(
                &mut rounded,
                BOX_X,
                BOX_Y,
                BOX_SIDE,
                BOX_SIDE,
                &CornerRadii::all(radius),
                ROUND_FG,
                1.0,
            );

            for y in 0..ROUND_SIDE {
                for x in 0..ROUND_SIDE {
                    assert_eq!(
                        rounded.get_pixel(x, y),
                        flat.get_pixel(x, y),
                        "radius {radius} changed the pixel at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// Rounding the corners must not disturb anything between them.
    ///
    /// The test above it cannot check this, and that is worth spelling out
    /// because it looks as though it does: a radius under half a pixel takes
    /// the flat path, so that test never enters the scanline code at all and
    /// no arithmetic error inside it can make that test fail. This one runs
    /// the rounded path and compares its straight middle against the flat
    /// primitive row for row — which is where a band boundary off by one
    /// shows up, as a seam along a window's side that no corner assertion
    /// would ever look at.
    #[test]
    fn rounding_a_corner_leaves_the_straight_edges_exactly_where_they_were() {
        let radius = 12.0_f32;
        let (engine, mut flat) = round_canvas();
        engine.fill_rect(&mut flat, BOX_X, BOX_Y, BOX_SIDE, BOX_SIDE, ROUND_FG, 1.0);

        let (engine, mut rounded) = round_canvas();
        engine.fill_round_rect(
            &mut rounded,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii::all(radius),
            ROUND_FG,
            1.0,
        );

        // Every row strictly between the two corner bands, full width.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let band = radius.ceil() as u32;
        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let by = BOX_Y as u32;
        let mut compared = 0_u32;
        // The full canvas width, not just the box's: a rounded fill that
        // spilled sideways would show up out here, where the flat one is bare.
        for y in by + band..by + BOX_SIDE - band {
            for x in 0..ROUND_SIDE {
                assert_eq!(
                    rounded.get_pixel(x, y),
                    flat.get_pixel(x, y),
                    "rounding the corners changed the pixel at ({x}, {y}), which \
                     lies on a straight side"
                );
                compared += 1;
            }
        }
        assert!(
            compared > 0,
            "the bands swallowed the whole shape; nothing was compared"
        );
    }

    /// The whole point: a corner asked to be round is not painted.
    #[test]
    fn a_rounded_fill_leaves_its_corners_bare() {
        let (engine, mut fb) = round_canvas();
        engine.fill_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii::all(12.0),
            ROUND_FG,
            1.0,
        );

        // Each of the four corner pixels of the bounding box.
        let far = BOX_SIDE - 1;
        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let (bx, by) = (BOX_X as u32, BOX_Y as u32);
        for (x, y) in [
            (bx, by),
            (bx + far, by),
            (bx, by + far),
            (bx + far, by + far),
        ] {
            assert_eq!(
                ink(&fb, x, y),
                0,
                "the corner pixel at ({x}, {y}) was painted by a rounded fill"
            );
        }

        // ...while the parts that are still square are solid.
        let mid = BOX_SIDE / 2;
        for (x, y, what) in [
            (bx + mid, by, "top edge"),
            (bx + mid, by + far, "bottom edge"),
            (bx, by + mid, "left edge"),
            (bx + far, by + mid, "right edge"),
            (bx + mid, by + mid, "centre"),
        ] {
            assert_eq!(ink(&fb, x, y), 0xFF, "the {what} should be solid");
        }
    }

    /// A corner has to be a quarter *circle*, not a chamfer.
    ///
    /// Checked by area rather than by re-deriving the arc: a test that
    /// recomputed `r - sqrt(r² - dy²)` would be a second copy of the code it
    /// is checking, and would agree with it however wrong it was. The area a
    /// quarter circle leaves out of its bounding square is `r²(1 - π/4)`,
    /// which a chamfer (`r²/2`) and a square corner (`0`) both miss by far
    /// more than the tolerance here.
    #[test]
    fn a_rounded_corner_is_a_quarter_circle_and_not_a_chamfer() {
        let radius = 16.0_f32;
        let (engine, mut fb) = round_canvas();
        engine.fill_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii::all(radius),
            ROUND_FG,
            1.0,
        );

        // Sum the *missing* coverage over the top-left corner's bounding box.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let r = radius as u32;
        let mut missing = 0.0_f32;
        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let (bx, by) = (BOX_X as u32, BOX_Y as u32);
        for y in by..by + r {
            for x in bx..bx + r {
                #[allow(clippy::cast_precision_loss, reason = "a byte of coverage")]
                let covered = ink(&fb, x, y) as f32 / 255.0;
                missing += 1.0 - covered;
            }
        }

        let expected = radius * radius * (1.0 - core::f32::consts::FRAC_PI_4);
        let chamfer = radius * radius / 2.0;
        assert!(
            (missing - expected).abs() < radius,
            "the corner cut out {missing:.1} px² where a quarter circle of \
             radius {radius} cuts out {expected:.1} (a chamfer would cut \
             {chamfer:.1}, a square corner 0)"
        );
    }

    /// The arc is smoothed, not stair-stepped.
    ///
    /// Without this, `fill_span_row` could round its span to whole pixels and
    /// every assertion above would still pass — the shape would be right and
    /// the edge would be visibly jagged.
    #[test]
    fn a_rounded_corner_is_antialiased() {
        let (engine, mut fb) = round_canvas();
        engine.fill_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii::all(12.0),
            ROUND_FG,
            1.0,
        );

        let partial = inked(&fb)
            .into_iter()
            .filter(|&(x, y)| {
                let level = ink(&fb, x, y);
                level > 0 && level < 0xFF
            })
            .count();
        assert!(
            partial >= 12,
            "only {partial} pixels along the arcs were partially covered; a \
             smoothed corner of this radius has one per scanline per corner"
        );
    }

    /// A client picks the radii and is not obliged to pick reasonable ones.
    ///
    /// The failure guarded against is not ugliness: an unclamped radius makes
    /// `r² - dy²` positive far outside the box, so the span walks off the
    /// shape, and an unchecked band height makes the middle quad's height
    /// underflow to roughly four billion rows.
    #[test]
    fn a_radius_larger_than_the_box_is_clamped_rather_than_escaping_it() {
        for radius in [f32::from(u16::MAX), 1.0e30] {
            let (engine, mut fb) = round_canvas();
            engine.fill_round_rect(
                &mut fb,
                BOX_X,
                BOX_Y,
                BOX_SIDE,
                BOX_SIDE,
                &CornerRadii::all(radius),
                ROUND_FG,
                1.0,
            );

            let centre = BOX_SIDE / 2;
            #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
            let (bx, by) = (BOX_X as u32, BOX_Y as u32);
            assert_eq!(
                ink(&fb, bx + centre, by + centre),
                0xFF,
                "radius {radius} erased the middle of the shape"
            );
            assert_eq!(ink(&fb, bx, by), 0, "radius {radius} left a square corner");
            for (x, y) in inked(&fb) {
                assert!(
                    x >= bx && x < bx + BOX_SIDE && y >= by && y < by + BOX_SIDE,
                    "radius {radius} painted ({x}, {y}), outside the box"
                );
            }
        }
    }

    /// A radius that is not a number, or not positive, must leave a square —
    /// never an empty hole where a window was.
    ///
    /// **Honest note on what this pins.** Deleting the `sane` filter in
    /// `RoundRect::new` does *not* make this fail, and that was checked rather
    /// than assumed. The degradation to a square is overdetermined: `f32::max`
    /// discards NaN in favour of its other operand, `NaN > 0.0` is false so
    /// the overlap clamp skips it, and `NaN as u32` saturates to zero, which
    /// collapses both corner bands and leaves the middle quad covering the
    /// whole shape. Three independent accidents arrive at the right answer.
    ///
    /// The filter stays anyway, because "right by accident along three paths
    /// at once" is not a property anyone can maintain — the next edit to any
    /// of those three would break it silently. So this test pins the
    /// user-visible behaviour, which is what actually matters, and the filter
    /// makes that behaviour deliberate. It is deliberately not claimed to be
    /// a regression test for the filter itself.
    #[test]
    fn a_nonsense_radius_falls_back_to_a_square_rather_than_vanishing() {
        for radius in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -8.0] {
            let (engine, mut fb) = round_canvas();
            engine.fill_round_rect(
                &mut fb,
                BOX_X,
                BOX_Y,
                BOX_SIDE,
                BOX_SIDE,
                &CornerRadii::all(radius),
                ROUND_FG,
                1.0,
            );
            #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
            let (bx, by) = (BOX_X as u32, BOX_Y as u32);
            assert_eq!(
                ink(&fb, bx, by),
                0xFF,
                "radius {radius} left the shape unpainted instead of square"
            );
            assert_eq!(
                inked(&fb).len(),
                (BOX_SIDE * BOX_SIDE) as usize,
                "radius {radius} did not draw the whole square"
            );
        }
    }

    /// An outline is hollow, and its corners are round.
    #[test]
    fn a_rounded_stroke_is_hollow_and_its_corners_are_bare() {
        let (engine, mut fb) = round_canvas();
        engine.stroke_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            3,
            &CornerRadii::all(12.0),
            ROUND_FG,
            1.0,
        );

        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let (bx, by) = (BOX_X as u32, BOX_Y as u32);
        let mid = BOX_SIDE / 2;
        assert_eq!(
            ink(&fb, bx + mid, by + mid),
            0,
            "the middle of an outline was filled in"
        );
        // Probed inside the *corner band* as well, not only the straight
        // middle. Those are two different code paths — the middle coalesces
        // into two bars without ever consulting the inner shape, so a stroke
        // that had forgotten to hollow itself out at all still left this
        // shape's centre bare and passed on that assertion alone.
        assert_eq!(
            ink(&fb, bx + mid, by + 6),
            0,
            "the outline was solid across a scanline that runs through its \
             rounded corners"
        );
        assert_eq!(ink(&fb, bx, by), 0, "the outline left a square corner");
        assert_eq!(
            ink(&fb, bx + mid, by),
            0xFF,
            "the top edge of the outline is missing"
        );
        assert_eq!(
            ink(&fb, bx, by + mid),
            0xFF,
            "the left edge of the outline is missing"
        );
        assert_eq!(
            ink(&fb, bx + BOX_SIDE - 1, by + mid),
            0xFF,
            "the right edge of the outline is missing"
        );
    }

    /// **The regression that matters for the pair.** The outline is drawn
    /// around the fill it belongs to, so every pixel the outline touches must
    /// be one the fill would have touched. If the two derive their curve
    /// differently they part company by a pixel somewhere along the arc, and
    /// the result is a border that floats free of its own window at the
    /// corners — the exact defect that made `stroke_round_rect` reuse
    /// `RoundRect::span` instead of growing arc code of its own.
    #[test]
    fn a_rounded_stroke_never_paints_outside_the_fill_it_surrounds() {
        let radii = CornerRadii {
            top_left: 14.0,
            top_right: 6.0,
            bottom_right: 18.0,
            bottom_left: 0.0,
        };

        let (engine, mut filled) = round_canvas();
        engine.fill_round_rect(
            &mut filled,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &radii,
            ROUND_FG,
            1.0,
        );

        let (engine, mut stroked) = round_canvas();
        engine.stroke_round_rect(
            &mut stroked,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            2,
            &radii,
            ROUND_FG,
            1.0,
        );

        let escaped: Vec<(u32, u32)> = inked(&stroked)
            .into_iter()
            .filter(|&(x, y)| ink(&filled, x, y) == 0)
            .collect();
        assert!(
            escaped.is_empty(),
            "the outline painted {} pixels the fill does not cover, e.g. {:?}",
            escaped.len(),
            &escaped[..escaped.len().min(6)]
        );
    }

    /// Asymmetric radii must be honoured per corner, not averaged into one.
    #[test]
    fn each_corner_takes_its_own_radius() {
        let (engine, mut fb) = round_canvas();
        engine.fill_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii {
                top_left: 16.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            ROUND_FG,
            1.0,
        );

        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let (bx, by) = (BOX_X as u32, BOX_Y as u32);
        let far = BOX_SIDE - 1;
        assert_eq!(
            ink(&fb, bx, by),
            0,
            "the rounded top-left corner was painted"
        );
        for (x, y, which) in [
            (bx + far, by, "top-right"),
            (bx, by + far, "bottom-left"),
            (bx + far, by + far, "bottom-right"),
        ] {
            assert_eq!(
                ink(&fb, x, y),
                0xFF,
                "the {which} corner asked to stay square and was rounded anyway"
            );
        }
    }

    /// **The bug itself.** Everything above tests the rasterizer; this tests
    /// that a client's radii ever reach it. They did not: `execute_command`
    /// bound `corner_radii: _` and called the flat primitives, so the whole
    /// rounded path could have existed and been correct and still never run.
    /// A test that only calls `fill_round_rect` directly would have passed
    /// against the broken tree.
    #[test]
    fn a_clients_corner_radii_reach_the_rasterizer() {
        let radii = CornerRadii::all(12.0);
        let white = Color::rgb(255, 255, 255);
        // Window-local coordinates: `execute` translates by the window origin.
        let commands = [
            RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
                color: white,
                corner_radii: radii,
            },
            RenderCommand::StrokeRect {
                x: 24.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
                color: white,
                line_width: 3.0,
                corner_radii: radii,
            },
        ];

        let (mut engine, mut fb) = round_canvas();
        engine.execute(&mut fb, &commands, BOX_X, BOX_Y, BOX_SIDE, BOX_SIDE, 1.0);

        #[allow(clippy::cast_sign_loss, reason = "the box is at a positive origin")]
        let (bx, by) = (BOX_X as u32, BOX_Y as u32);
        assert_eq!(
            ink(&fb, bx, by),
            0,
            "a client's FillRect radii were discarded and the corner drawn square"
        );
        assert_eq!(
            ink(&fb, bx + 24, by),
            0,
            "a client's StrokeRect radii were discarded and the corner drawn square"
        );
        // ...and the shapes were actually drawn, so that "no ink in the
        // corner" cannot be satisfied by drawing nothing at all.
        assert_eq!(ink(&fb, bx + 10, by + 10), 0xFF, "the fill is missing");
        assert_eq!(ink(&fb, bx + 34, by), 0xFF, "the outline is missing");
    }

    /// A new primitive is a new chance to paint straight at the framebuffer
    /// and forget the clip stack, which is how a menu escapes its own window.
    #[test]
    fn a_rounded_fill_still_obeys_the_clip_stack() {
        let (mut engine, mut fb) = round_canvas();
        let clip = Rect::new(BOX_X, BOX_Y, BOX_SIDE / 2, BOX_SIDE);
        engine.clip_stack.push(clip);
        engine.fill_round_rect(
            &mut fb,
            BOX_X,
            BOX_Y,
            BOX_SIDE,
            BOX_SIDE,
            &CornerRadii::all(10.0),
            ROUND_FG,
            1.0,
        );
        engine.clip_stack.pop();

        for (x, y) in inked(&fb) {
            assert!(
                clip.contains(x as i32, y as i32),
                "a rounded fill painted ({x}, {y}), outside its clip {clip:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The user's appearance settings reaching the decorations
    //
    // The compositor drew its own window frames from a hardcoded
    // `DecorationTheme` and nothing else. Two of the user's choices in the
    // Settings app — how round window corners are, and whether windows cast
    // shadows — had no route into the process that draws the frames, so both
    // were ignored outright: `Square` and `ExtraRounded` produced the same
    // square frame, and turning shadows off left every shadow exactly where it
    // was. These tests assert on the composited pixels, because "the field is
    // stored" is precisely what was true before and was not enough.
    // -----------------------------------------------------------------------

    /// The display these tests composite onto. Large enough that a window can
    /// sit well inside it with room for its shadow to fall on the desktop.
    const DECOR_W: u32 = 400;
    /// See [`DECOR_W`].
    const DECOR_H: u32 = 300;

    /// A compositor with one decorated window at a known place, rendered once
    /// onto a cleared desktop under the given appearance settings.
    ///
    /// Renders one window rather than composing a frame so that what lands in
    /// the buffer is exactly this window's decorations over a known background
    /// — the same idiom as `a_scaled_shadow_is_actually_drawn_to_its_scaled_extent`.
    fn decorated(settings: AppearanceSettings) -> (Compositor, WindowId) {
        let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
        comp.set_appearance(settings);
        let mut spec = WindowSpec::new("Framed", 160, 120);
        spec.position = Some((120, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.refresh_window_scales();
        let bg = comp.theme.desktop_background;
        comp.backend.clear(bg);
        comp.render_window(id);
        (comp, id)
    }

    /// Settings that differ from the defaults only in their corner style.
    fn with_corners(corners: WindowCorners) -> AppearanceSettings {
        AppearanceSettings {
            window_corners: corners,
            ..AppearanceSettings::default()
        }
    }

    /// How many pixels of `rect`'s top-left `size`×`size` block are painted in
    /// exactly `color`.
    ///
    /// A count over a block rather than a probe at one coordinate, because it
    /// measures *how much* of the corner was cut away. That is what lets one
    /// measurement tell `Subtle` from `ExtraRounded`, without this test carrying
    /// a second copy of the arc arithmetic that could agree with a wrong
    /// original. Exact equality is deliberate: the antialiased rim pixels are
    /// blends and do not count, so what is counted is the solid interior, which
    /// is unambiguous.
    ///
    /// It is a *relative* measure only. A block over a window frame also
    /// contains the border stroke down its left edge and the first letters of
    /// the title, neither of which is frame colour — so the count sits well
    /// below the block's area even for a perfectly square corner (339 of 400,
    /// when this was written). Those contaminants do not move with the corner
    /// setting, which is why comparing two counts over the same block is sound
    /// and asserting an absolute one is not.
    fn corner_ink(comp: &Compositor, rect: Rect, size: u32, color: u32) -> u32 {
        let mut count: u32 = 0;
        for dy in 0..size {
            for dx in 0..size {
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "the sampled block is inside the window, which is \
                              inside the buffer"
                )]
                let (x, y) = ((rect.x + dx as i32) as u32, (rect.y + dy as i32) as u32);
                if working_pixel(&comp.backend, x, y) == Some(color) {
                    count = count.saturating_add(1);
                }
            }
        }
        count
    }

    /// The title bar's own rectangle and background colour, for a focused
    /// window. Read from the window rather than recomputed, so a change to the
    /// layout cannot leave these tests probing empty desktop.
    fn title_bar_of(comp: &Compositor, id: WindowId) -> (Rect, u32) {
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_layout()
            .expect("a decorated window has a title bar");
        let focused = comp.window_ref(id).expect("window").focused;
        let color = if focused {
            comp.theme.title_bar_focused
        } else {
            comp.theme.title_bar_unfocused
        };
        (bar.bar, color)
    }

    /// How big a block to measure a corner over. Larger than the largest radius
    /// the settings offer (16), so `ExtraRounded` still leaves solid pixels in
    /// the block and the counts stay comparable.
    const CORNER_BLOCK: u32 = 20;

    #[test]
    fn the_users_corner_setting_reaches_the_window_frame() {
        // The bug: it did not. Every window was drawn with a square frame no
        // matter what the user chose, because the compositor had no connection
        // to the appearance settings at all — `Square` and `ExtraRounded` were
        // the same picture.
        //
        // Probed two pixels in from the frame's top-left rather than counted
        // over a block, because this one pixel is unambiguous: it is inside the
        // border stroke and well clear of the title text, so the only thing
        // that decides its colour is whether the corner was cut away. A square
        // corner paints it; a 16px arc is still 8px away from it at that depth.
        let corner_painted = |corners| {
            let (comp, id) = decorated(with_corners(corners));
            let (bar, color) = title_bar_of(&comp, id);
            #[allow(
                clippy::cast_sign_loss,
                reason = "the window is placed well inside the 400x300 buffer"
            )]
            let probe = ((bar.x + 2) as u32, (bar.y + 2) as u32);
            working_pixel(&comp.backend, probe.0, probe.1) == Some(color)
        };
        assert!(
            corner_painted(WindowCorners::Square),
            "a square corner left its own corner pixel unpainted"
        );
        assert!(
            !corner_painted(WindowCorners::ExtraRounded),
            "the user asked for extra-rounded windows and the corner pixel was \
             painted anyway — the setting never reached the frame"
        );
    }

    #[test]
    fn a_deeper_corner_setting_cuts_more_of_the_corner_away() {
        // Not just "some rounding happened": the *amount* has to follow the
        // setting. A compositor that rounded every window by one hardcoded
        // radius would pass the test above and fail this one, and would ignore
        // three of the four choices the settings panel offers.
        let inks: Vec<(WindowCorners, u32)> = [
            WindowCorners::Square,
            WindowCorners::Subtle,
            WindowCorners::Rounded,
            WindowCorners::ExtraRounded,
        ]
        .into_iter()
        .map(|corners| {
            let (comp, id) = decorated(with_corners(corners));
            let (bar, color) = title_bar_of(&comp, id);
            (corners, corner_ink(&comp, bar, CORNER_BLOCK, color))
        })
        .collect();

        for pair in inks.windows(2) {
            let [(shallow, more), (deep, less)] = pair else {
                unreachable!("windows(2) yields pairs")
            };
            assert!(
                less < more,
                "{deep:?} (radius {}) left {less} painted pixels, which is not fewer \
                 than {shallow:?} (radius {}) at {more}",
                deep.radius(),
                shallow.radius(),
            );
        }
    }

    #[test]
    fn the_border_rounds_with_the_frame_it_traces() {
        // The border is a separate call from the title bar, so rounding one
        // does not round the other — and a square border around a rounded frame
        // is not a subtle defect: it is a hard rectangular outline standing off
        // the window's curved corners with desktop showing between them.
        let border_at_the_corner = |corners| {
            let (comp, id) = decorated(with_corners(corners));
            let frame = comp.window_ref(id).expect("window").frame_rect();
            let width = scale_dimension(
                BORDER_WIDTH,
                comp.window_ref(id).expect("window").scale_factor,
            );
            // The border box starts one stroke above the frame; see
            // `render_border`. Its own top-left is the pixel a square stroke
            // paints and a rounded one leaves alone.
            #[allow(
                clippy::cast_sign_loss,
                reason = "the window is placed well inside the 400x300 buffer"
            )]
            let probe = (frame.x as u32, (frame.y - width as i32) as u32);
            let focused = comp.window_ref(id).expect("window").focused;
            let expected = if focused {
                comp.theme.border_focused
            } else {
                comp.theme.border_unfocused
            };
            working_pixel(&comp.backend, probe.0, probe.1) == Some(expected)
        };
        assert!(
            border_at_the_corner(WindowCorners::Square),
            "a square border did not paint its own corner pixel"
        );
        assert!(
            !border_at_the_corner(WindowCorners::ExtraRounded),
            "the border stayed square while the frame rounded"
        );
    }

    #[test]
    fn the_shadow_rounds_with_the_window_it_falls_from() {
        // A shadow is the window's own silhouette, offset and blurred. Rings
        // that kept square corners under a rounded window would show as dark
        // right angles poking diagonally out past the curve they are supposed
        // to be sitting behind — the one place a shadow is most visible.
        let shadow_ink = |corners| {
            let (comp, id) = decorated(with_corners(corners));
            let frame = comp.window_ref(id).expect("window").frame_rect();
            let bg = comp.theme.desktop_background;
            // The block diagonally off the frame's bottom-right, which is
            // shadow and nothing else: the window does not reach it and the
            // desktop behind it is a flat colour.
            #[allow(
                clippy::cast_sign_loss,
                reason = "the window is placed well inside the 400x300 buffer"
            )]
            let origin = (
                (frame.x + frame.width as i32) as u32,
                (frame.y + frame.height as i32) as u32,
            );
            let mut painted = 0u32;
            for dy in 0..SHADOW_SIZE {
                for dx in 0..SHADOW_SIZE {
                    let (x, y) = (origin.0.saturating_add(dx), origin.1.saturating_add(dy));
                    if working_pixel(&comp.backend, x, y).is_some_and(|p| p != bg) {
                        painted = painted.saturating_add(1);
                    }
                }
            }
            painted
        };
        let square = shadow_ink(WindowCorners::Square);
        let rounded = shadow_ink(WindowCorners::ExtraRounded);
        assert!(square > 0, "no shadow was drawn at all beside the window");
        assert!(
            rounded < square,
            "the shadow's corner stayed square ({rounded} painted pixels) under a \
             rounded window; the square one painted {square}"
        );
    }

    #[test]
    fn the_corner_radius_grows_with_the_display_scale() {
        // Every other decoration dimension is scaled to the display
        // (`scale_dimension`). A radius that was not would leave a 2x window —
        // twice the frame, twice the title bar, twice the border — wearing the
        // same 8px curve, which reads as a window with sharper corners rather
        // than as the same window drawn larger.
        //
        // Measured as the depth at which the title bar's top row starts being
        // painted, rather than predicted from the radius: re-deriving the arc
        // here would be a second copy of it that can agree with a wrong
        // original.
        let corner_depth = |scale: f32| {
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
            if let Some(d) = comp.display_manager.displays.first_mut() {
                d.scale_factor = scale;
            }
            comp.set_appearance(with_corners(WindowCorners::ExtraRounded));
            let mut spec = WindowSpec::new("Framed", 160, 120);
            spec.position = Some((120, 100));
            let id = comp.create_window_from_spec(&spec, 1);
            comp.refresh_window_scales();
            comp.backend.clear(comp.theme.desktop_background);
            comp.render_window(id);

            let (bar, color) = title_bar_of(&comp, id);
            #[allow(
                clippy::cast_sign_loss,
                reason = "the window is placed well inside the 400x300 buffer"
            )]
            let row = bar.y as u32;
            #[allow(
                clippy::cast_sign_loss,
                reason = "the window is placed well inside the 400x300 buffer"
            )]
            let left = bar.x as u32;
            (0..bar.width)
                .find(|&dx| {
                    working_pixel(&comp.backend, left.saturating_add(dx), row) == Some(color)
                })
                .expect("the title bar's top row is painted somewhere")
        };
        let single = corner_depth(1.0);
        let double = corner_depth(2.0);
        assert!(
            single > 0,
            "an extra-rounded corner cut nothing from the top row"
        );
        assert!(
            double > single,
            "the corner bit {double} pixels into the top row at 2x and {single} at \
             1x — the radius did not scale with the display"
        );
    }

    #[test]
    fn the_window_buttons_round_with_the_windows() {
        // A square close button beside a curved frame corner is the mismatch,
        // not the consistency — and the buttons are drawn by a different call
        // than the frame, so rounding one does not round the other.
        let button_of = |corners| {
            let (comp, id) = decorated(with_corners(corners));
            let close = comp
                .window_ref(id)
                .expect("window")
                .close_button_rect()
                .expect("an ordinary window has a close button");
            let ink = corner_ink(
                &comp,
                close,
                close.width.min(close.height),
                comp.theme.close_button,
            );
            (close, ink)
        };
        let (square_rect, square_ink) = button_of(WindowCorners::Square);
        let (round_rect, round_ink) = button_of(WindowCorners::ExtraRounded);
        assert_eq!(
            square_rect, round_rect,
            "the button geometry must not move, or the two counts are of different things"
        );
        assert!(
            round_ink < square_ink,
            "the close button stayed square ({round_ink} painted pixels) while the \
             frame rounded; square's was {square_ink}"
        );
    }

    #[test]
    fn turning_drop_shadows_off_leaves_the_desktop_beside_the_window_bare() {
        let settings = |drop_shadows| AppearanceSettings {
            drop_shadows,
            ..AppearanceSettings::default()
        };
        let (with, id) = decorated(settings(true));
        let (without, _) = decorated(settings(false));

        let frame = with.window_ref(id).expect("window").frame_rect();
        let bg = with.theme.desktop_background;
        // A row through the middle of the window, sampled to the right of the
        // frame: shadow country, and nothing else is drawn out there.
        #[allow(
            clippy::cast_sign_loss,
            reason = "the window is placed well inside the 400x300 buffer"
        )]
        let row = (frame.y + frame.height as i32 / 2) as u32;
        #[allow(
            clippy::cast_sign_loss,
            reason = "the window is placed well inside the 400x300 buffer"
        )]
        let right = (frame.x + frame.width as i32) as u32;
        let band = right..right.saturating_add(SHADOW_SIZE);

        let shaded = band
            .clone()
            .filter(|&x| working_pixel(&with.backend, x, row) != Some(bg))
            .count();
        assert!(
            shaded > 0,
            "with shadows on, nothing was painted in the shadow band — this test \
             cannot tell suppression from an empty band"
        );
        let unshaded = band
            .filter(|&x| working_pixel(&without.backend, x, row) != Some(bg))
            .count();
        assert_eq!(
            unshaded, 0,
            "the user turned drop shadows off and {unshaded} shadow pixels were \
             still painted beside the window"
        );
    }

    #[test]
    fn a_maximized_window_casts_no_shadow_it_could_only_smear_over_itself() {
        // A maximized frame is fitted to the display exactly, so every ring of
        // its shadow is clipped away or drawn under the window's own frame. On
        // an opaque window that is pure overdraw; on a translucent one the
        // rings show through as a dark smear along the top and left edges,
        // which is what this test can see. The comparison is against the same
        // scene with shadows turned off: if the suppression works, maximizing
        // is indistinguishable from having asked for no shadows at all.
        let scene = |drop_shadows| {
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
            comp.set_appearance(AppearanceSettings {
                drop_shadows,
                ..AppearanceSettings::default()
            });
            let id = comp.create_window("Framed".to_string(), 160, 120, 1);
            // Translucent, so the frame blends over whatever is beneath it
            // instead of hiding it. An opaque maximized window would look
            // identical either way and prove nothing.
            comp.set_opacity(id, 0.5).expect("opacity");
            comp.maximize_window(id).expect("maximize");
            comp.refresh_window_scales();
            comp.backend.clear(comp.theme.desktop_background);
            comp.render_window(id);
            comp.backend.working_pixels().to_vec()
        };
        assert_eq!(
            scene(true),
            scene(false),
            "a maximized window was drawn differently with shadows on than with \
             them off, which means a shadow it can only smear over itself was drawn"
        );
    }

    #[test]
    fn an_ordinary_window_still_casts_one() {
        // Non-vacuity for the test above: the comparison it makes must be
        // capable of coming out unequal.
        let scene = |drop_shadows| {
            let (comp, _) = decorated(AppearanceSettings {
                drop_shadows,
                ..AppearanceSettings::default()
            });
            comp.backend.working_pixels().to_vec()
        };
        assert_ne!(
            scene(true),
            scene(false),
            "an ordinary window looked the same with shadows on and off"
        );
    }

    #[test]
    fn changing_the_appearance_repaints_what_is_already_on_screen() {
        // A settings change moves pixels that no window's damage covers: the
        // quarter-disc a squared corner reclaims, and the strip a removed
        // shadow vacates. Nothing marks those dirty, so without a forced
        // recomposite the user changes the setting and the screen does not
        // change until something else happens to repaint that area.
        //
        // A refresh rate past 1 MHz, because `frame_interval_for` divides into
        // it and lands on a zero-length frame budget: `compose_frame` also
        // declines when it is *too soon* for another frame, and at 60 Hz three
        // calls in a row are all inside one 16 ms interval — so the assertions
        // below would be reading the vsync gate rather than the damage state,
        // and the middle one would pass for entirely the wrong reason.
        let mut comp = Compositor::new(DECOR_W, DECOR_H, 2_000_000).expect("compositor");
        comp.create_window("Framed".to_string(), 160, 120, 1);
        assert!(comp.compose_frame(), "the first frame draws the new window");
        assert!(
            !comp.compose_frame(),
            "an unchanged desktop should have nothing to redraw"
        );
        comp.set_appearance(with_corners(WindowCorners::Square));
        assert!(
            comp.compose_frame(),
            "the corner setting changed and nothing was redrawn"
        );
    }

    // -----------------------------------------------------------------------
    // The user's colours and title font reaching the decorations
    //
    // Corners and shadows arrived first because they are *shapes*; the colours
    // stayed behind in twelve constants at the top of this file, and the title
    // font in one. So a user in light mode got a dark-navy desktop and a
    // blue-gray title bar from the process that actually draws them, a user
    // with accented title bars got the same blue-gray, and a user who enlarged
    // the interface font because they could not read it got window titles at
    // 16px regardless. Each of those is a difference the desktop shell's own
    // (duplicate) decorator got right, which is what made the divergence
    // visible: the two renderers disagreed about the same window.
    //
    // These tests take their expected values from `appearance`, never from
    // `comp.theme` — reading the answer back out of the thing under test is
    // exactly what the old hardcoded palette would also have passed.
    // -----------------------------------------------------------------------

    /// Settings that differ from the defaults only in their theme mode.
    fn with_mode(mode: appearance::ThemeMode) -> AppearanceSettings {
        AppearanceSettings {
            theme_mode: mode,
            ..AppearanceSettings::default()
        }
    }

    #[test]
    fn the_users_theme_mode_reaches_the_desktop_behind_the_windows() {
        // The clearest single pixel in the whole increment: the desktop, where
        // nothing covers it. It was `0xFF1A1A2E` — a dark navy that appears in
        // neither palette — in light mode and in dark mode alike.
        let painted = |mode| {
            let (comp, _) = decorated(with_mode(mode));
            // Top-left, far from the window at (120, 100) and outside its
            // shadow.
            working_pixel(&comp.backend, 4, 4)
        };
        let expected = |light| {
            Some(color_to_argb(
                &appearance::DecorationColors::for_mode(light).desktop_bg,
            ))
        };

        assert_eq!(
            painted(appearance::ThemeMode::Light),
            expected(true),
            "a user in light mode got a desktop colour that is not the light \
             palette's"
        );
        assert_eq!(
            painted(appearance::ThemeMode::Dark),
            expected(false),
            "a user in dark mode got a desktop colour that is not the dark \
             palette's"
        );
        assert_ne!(
            painted(appearance::ThemeMode::Light),
            painted(appearance::ThemeMode::Dark),
            "both modes produced the same desktop — a compositor that ignores \
             the mode passes the two assertions above only if the palettes are \
             equal, which they are not"
        );
    }

    #[test]
    fn the_users_accent_reaches_the_title_bar_it_asked_for() {
        // `accent_titlebars` is a checkbox in the Settings app that, for the
        // process drawing the title bars, did nothing whatsoever.
        let settings = AppearanceSettings {
            accent_titlebars: true,
            accent_color: appearance::AccentColor::Red,
            ..AppearanceSettings::default()
        };
        let accent = color_to_argb(&settings.effective_accent());
        let (comp, id) = decorated(settings);

        // Sampled at the vertical middle of the bar and to the right of the
        // title text but left of the buttons, so the only thing that can be
        // painted there is the bar's own background.
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_layout()
            .expect("a decorated window has a title bar")
            .bar;
        #[allow(
            clippy::cast_sign_loss,
            reason = "the window is placed well inside the 400x300 buffer"
        )]
        let (x, y) = (
            (bar.x + bar.width as i32 / 2) as u32,
            (bar.y + bar.height as i32 / 2) as u32,
        );

        assert_eq!(
            working_pixel(&comp.backend, x, y),
            Some(accent),
            "the user asked for accent-coloured title bars and the focused \
             window's bar is not the accent colour"
        );
    }

    #[test]
    fn an_accent_title_bar_leaves_every_other_window_alone() {
        // The unfocused bar keeps the base palette on purpose: an accent that
        // marks every window marks none of them, and telling the focused window
        // apart is the title bar's first job. Stated as a test because it is a
        // one-line difference in `DecorationColors::from_settings` that a later
        // "apply the accent consistently" tidy-up would quietly remove.
        let plain = AppearanceSettings::default();
        let accented = AppearanceSettings {
            accent_titlebars: true,
            accent_color: appearance::AccentColor::Red,
            ..AppearanceSettings::default()
        };
        let accent = color_to_argb(&accented.effective_accent());

        let with = DecorationTheme::from_settings(&accented);
        let without = DecorationTheme::from_settings(&plain);

        assert_eq!(
            with.title_bar_unfocused, without.title_bar_unfocused,
            "turning on accented title bars recoloured the unfocused ones too"
        );
        assert_ne!(
            with.title_bar_focused, without.title_bar_focused,
            "turning on accented title bars did nothing to the focused one — \
             the assertion above would then hold for the wrong reason"
        );
        assert_eq!(
            with.title_bar_focused, accent,
            "the focused bar changed to something that is not the accent"
        );
    }

    #[test]
    fn the_users_interface_font_size_reaches_the_window_title() {
        // The title was drawn at a constant 16px. Someone who enlarged the UI
        // font — the one setting a person with poor eyesight is most likely to
        // reach for — got every part of the desktop bigger except the titles.
        //
        // Counted rather than probed: where a glyph's ink lands depends on the
        // font, but *how much* of it there is has to grow with the size. What
        // is counted is every pixel of the bar that is not the bar's own
        // background — glyphs are antialiased, so their edge pixels are blends
        // and matching the text colour exactly would find almost none of them.
        // The buttons and the rounded corners are counted too, but they do not
        // move with the font size, which is why comparing two counts is sound
        // and asserting an absolute one is not.
        let bar_ink = |title: &str, ui_size: f32| {
            let mut settings = AppearanceSettings::default();
            settings.fonts.ui_size = ui_size;
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
            comp.set_appearance(settings);
            let mut spec = WindowSpec::new(title, 160, 120);
            spec.position = Some((120, 100));
            let id = comp.create_window_from_spec(&spec, 1);
            comp.refresh_window_scales();
            comp.backend.clear(comp.theme.desktop_background);
            comp.render_window(id);

            let bar = comp
                .window_ref(id)
                .expect("window")
                .title_bar_layout()
                .expect("a decorated window has a title bar")
                .bar;
            let bg = comp.theme.title_bar_focused;
            let mut count: u32 = 0;
            for dy in 0..bar.height {
                for dx in 0..bar.width {
                    #[allow(
                        clippy::cast_sign_loss,
                        reason = "the window is placed well inside the 400x300 buffer"
                    )]
                    let (x, y) = ((bar.x + dx as i32) as u32, (bar.y + dy as i32) as u32);
                    if working_pixel(&comp.backend, x, y) != Some(bg) {
                        count = count.saturating_add(1);
                    }
                }
            }
            count
        };

        // The same bar with nothing written on it, to establish that what grows
        // below is the writing and not the furniture around it.
        let blank = bar_ink("", 8.0);
        let small = bar_ink("Framed", 8.0);
        let large = bar_ink("Framed", 24.0);
        assert!(
            small > blank,
            "an empty title and a six-letter one inked the same {small} pixels — \
             this test is measuring the buttons, not the text"
        );
        assert!(
            large > small,
            "the user tripled the interface font size and the window title went \
             from {small} inked pixels to {large}"
        );
    }

    // -----------------------------------------------------------------------
    // Live reload
    //
    // Reading the settings once at startup left the user changing a setting and
    // watching nothing happen until they logged out. These tests cover the
    // reload path itself; that a wire request reaches it is covered in
    // `wire.rs`, where both halves of the protocol are in scope.
    // -----------------------------------------------------------------------

    /// Write `settings` to the scratch configuration directory as the user's
    /// own `appearance.yaml`, through the same type the Settings app saves
    /// with.
    ///
    /// Deliberately not a hand-written YAML literal: the file's key spellings
    /// belong to `gui/appearance`, and a literal here would be a second copy of
    /// them that could agree with a wrong reader or drift from a renamed key
    /// and fail for a reason that has nothing to do with the compositor.
    fn save_user_appearance(settings: AppearanceSettings) {
        let mut file = appearance::AppearanceFile::new();
        file.settings = settings;
        file.save().expect("write scratch appearance.yaml");
    }

    #[test]
    fn a_reload_adopts_what_the_users_file_now_says() {
        // The gap this closes: the settings were read once, in `main`, before
        // the first frame. Everything after that ran on whatever the file said
        // at login, so the Settings app could write a change the running
        // compositor would never see.
        appearance::config::testing::with_scratch_config("compositor-reload", |_root| {
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
            assert_eq!(
                comp.appearance().window_corners,
                WindowCorners::Rounded,
                "the constructor should start from the defaults, not the disk"
            );

            save_user_appearance(AppearanceSettings {
                window_corners: WindowCorners::Square,
                drop_shadows: false,
                ..AppearanceSettings::default()
            });
            comp.reload_appearance();

            assert_eq!(
                comp.appearance().window_corners,
                WindowCorners::Square,
                "the corner setting written to the file did not reach the compositor"
            );
            assert!(
                !comp.appearance().drop_shadows,
                "the shadow setting written to the file did not reach the compositor"
            );
        });
    }

    #[test]
    fn a_reload_that_changed_something_repaints_the_whole_screen() {
        // The same argument as `changing_the_appearance_repaints_what_is_
        // already_on_screen`, asserted through the reload path: the pixels a
        // corner or shadow change moves are outside every window's damage, so
        // without a forced recomposite the file changes and the screen does
        // not.
        appearance::config::testing::with_scratch_config("compositor-reload-damage", |_root| {
            // See `changing_the_appearance_repaints_what_is_already_on_screen`
            // for why the refresh rate is absurd: at 60 Hz these three calls
            // fall inside one frame interval and would measure the vsync gate.
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 2_000_000).expect("compositor");
            comp.create_window("Framed".to_string(), 160, 120, 1);
            assert!(comp.compose_frame(), "the first frame draws the new window");
            assert!(
                !comp.compose_frame(),
                "an unchanged desktop should have nothing to redraw"
            );

            save_user_appearance(with_corners(WindowCorners::Square));
            comp.reload_appearance();

            assert!(
                comp.compose_frame(),
                "the reload changed the corners and nothing was redrawn"
            );
        });
    }

    #[test]
    fn a_reload_that_finds_nothing_changed_costs_nothing() {
        // Any client that can open the display socket can ask for a reload, as
        // often as it likes. If each one repainted the screen, a request whose
        // whole safety argument is that it carries no data would still be a way
        // to hold the compositor at a full-screen redraw indefinitely. So the
        // damage state must survive a reload that read the same settings back.
        appearance::config::testing::with_scratch_config("compositor-reload-noop", |_root| {
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 2_000_000).expect("compositor");
            comp.create_window("Framed".to_string(), 160, 120, 1);
            assert!(comp.compose_frame(), "the first frame draws the new window");

            // No file written at all: a fresh install, where `load` yields the
            // defaults the compositor is already holding.
            for _ in 0..3 {
                comp.reload_appearance();
                assert!(
                    !comp.compose_frame(),
                    "a reload that changed nothing forced a full repaint"
                );
            }
        });
    }

    /// Write an `input.yaml` into the scratch configuration directory.
    ///
    /// Through `inputsettings` rather than as a YAML literal, for the same
    /// reason as `save_user_appearance` above: the key spellings belong to that
    /// crate, and a literal here would be a second copy of them.
    fn save_user_input(settings: inputsettings::InputSettings) {
        let mut file = inputsettings::InputFile::new();
        file.settings = settings;
        file.save().expect("write scratch input.yaml");
    }

    #[test]
    fn an_input_reload_adopts_the_double_click_speed_from_the_users_file() {
        // The gap this closes is not that the value was read at the wrong time
        // — it is that nothing read it at all. The mouse settings panel had a
        // double-click control with clamps and a renderer and no file behind
        // it, and the compositor had a hard-coded 400 ms. See `known-issues.md`
        // `TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING`.
        inputsettings::config::testing::with_scratch_config("compositor-reload-input", |_root| {
            let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
            assert_eq!(
                comp.double_click_ms(),
                400,
                "the constructor should start from the defaults, not the disk"
            );

            let mut settings = inputsettings::InputSettings::default();
            settings.mouse.set_double_click_ms(900);
            save_user_input(settings);
            comp.reload_input();

            assert_eq!(
                comp.double_click_ms(),
                900,
                "the double-click speed written to the file did not reach the compositor"
            );
        });
    }

    #[test]
    fn an_input_reload_never_repaints() {
        // No pixel depends on how long a double click may take, so a reload
        // that any client can send at any rate must not be able to hold the
        // compositor at a full-screen redraw — the same argument as
        // `a_reload_that_finds_nothing_changed_costs_nothing`, except that here
        // it holds even when the value *did* change.
        inputsettings::config::testing::with_scratch_config(
            "compositor-reload-input-quiet",
            |_root| {
                let mut comp = Compositor::new(DECOR_W, DECOR_H, 2_000_000).expect("compositor");
                comp.create_window("Framed".to_string(), 160, 120, 1);
                assert!(comp.compose_frame(), "the first frame draws the new window");

                let mut settings = inputsettings::InputSettings::default();
                settings.mouse.set_double_click_ms(1500);
                save_user_input(settings);

                for _ in 0..3 {
                    comp.reload_input();
                    assert!(
                        !comp.compose_frame(),
                        "an input reload forced a repaint of a screen it cannot change"
                    );
                }
                assert_eq!(comp.double_click_ms(), 1500);
            },
        );
    }

    #[test]
    fn an_input_reload_with_no_file_leaves_the_defaults_in_force() {
        // A fresh install, where the user has never opened the Mouse page.
        // Reading a missing file must not reset the interval to zero — which
        // would make a double click impossible to perform — nor fail.
        inputsettings::config::testing::with_scratch_config(
            "compositor-reload-input-missing",
            |_root| {
                let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
                comp.set_double_click_ms(250);
                comp.reload_input();
                assert_eq!(
                    comp.double_click_ms(),
                    400,
                    "a missing input.yaml should read as the defaults, not as nothing"
                );
            },
        );
    }

    #[test]
    fn a_hand_edited_input_file_cannot_make_a_double_click_impossible() {
        // The file is user-editable and the compositor is not the only thing
        // that will ever write it. A zero here would mean no two clicks are
        // ever close enough together, i.e. a title bar that cannot be
        // double-clicked at all — so it is clamped on the way in.
        //
        // What this proves, exactly: that *something* on the path from the file
        // to the interval refuses the zero. It cannot say which, and it is
        // worth being precise rather than claiming more than it shows — there
        // are two clamps here, `InputSettings::read_from` and then
        // `set_double_click_ms`, and the first alone is enough to make this
        // assertion hold. Deleting the compositor's own clamp leaves this test
        // green; what catches that is
        // `the_double_click_interval_is_clamped_to_a_performable_range`, which
        // calls the setter directly. The pair is deliberate — the second clamp
        // is what defends the reload path if the value ever arrives from
        // somewhere that has not been through `inputsettings` — so both tests
        // have to exist, and neither should be read as covering the other.
        inputsettings::config::testing::with_scratch_config(
            "compositor-reload-input-absurd",
            |root| {
                let path =
                    inputsettings::config::testing::scratch_path(root, inputsettings::CONFIG_NAME);
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(&path, "buttons:\n  double_click_ms: 0\n").expect("write");

                let mut comp = Compositor::new(DECOR_W, DECOR_H, 60).expect("compositor");
                comp.reload_input();
                assert_eq!(comp.double_click_ms(), MIN_DOUBLE_CLICK_MS);
            },
        );
    }

    // ---- drag a window to an edge and let go ------------------------------
    //
    // The compositor decides this, not the shell. It already holds the drag
    // grab, the window geometry and the display bounds, so a shell would have
    // to be sent the pointer position on every motion event to answer a
    // question the compositor can answer locally -- putting a socket round trip
    // inside the highlight, which is the part of the gesture the user watches.

    /// Drag `id` by its title bar until the pointer reaches `(to_x, to_y)`,
    /// then let go: the whole gesture, in the order a user performs it.
    fn drag_title_bar_to(comp: &mut Compositor, id: WindowId, to_x: i32, to_y: i32) {
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("a decorated window has a title bar");
        let from_x = bar.x + 10;
        let from_y = bar.y + bar.height as i32 / 2;
        comp.handle_mouse_button(MouseButton::Left, true, from_x, from_y);
        assert!(comp.drag.is_some(), "the title-bar press started no drag");
        comp.handle_mouse_move(to_x, to_y);
        comp.handle_mouse_button(MouseButton::Left, false, to_x, to_y);
        assert!(comp.drag.is_none(), "the release left the drag running");
    }

    fn snapped_to(comp: &Compositor, id: WindowId) -> Option<SnapTarget> {
        comp.window_ref(id).expect("window").snapped
    }

    /// The slot an edge drop is expected to land in.
    fn slot(preset: SnapLayoutPreset, zone: u8) -> SnapSlot {
        SnapSlot::new(preset, zone).expect("a zone the preset has")
    }

    #[test]
    fn dropping_a_window_at_an_edge_tiles_it_on_that_side() {
        // Every snapping edge and corner, driven end to end through the real
        // input path. Checking `snapped` rather than the rectangle is
        // deliberate: the stored value is the *intent*, and it is what survives
        // a resolution change, so a drop that produced the right pixels while
        // recording the wrong slot would still be wrong.
        let cases = [
            (
                "left",
                (2, 300),
                SnapTarget::Zone(slot(SnapLayoutPreset::TwoEqualHalves, 0)),
            ),
            (
                "right",
                (797, 300),
                SnapTarget::Zone(slot(SnapLayoutPreset::TwoEqualHalves, 1)),
            ),
            (
                "top-left",
                (2, 2),
                SnapTarget::Zone(slot(SnapLayoutPreset::FourQuadrants, 0)),
            ),
            (
                "top-right",
                (797, 2),
                SnapTarget::Zone(slot(SnapLayoutPreset::FourQuadrants, 1)),
            ),
            (
                "bottom-left",
                (2, 597),
                SnapTarget::Zone(slot(SnapLayoutPreset::FourQuadrants, 2)),
            ),
            (
                "bottom-right",
                (797, 597),
                SnapTarget::Zone(slot(SnapLayoutPreset::FourQuadrants, 3)),
            ),
        ];
        for (name, (x, y), want) in cases {
            let (mut comp, id) = with_one_window();
            drag_title_bar_to(&mut comp, id, x, y);
            assert_eq!(
                snapped_to(&comp, id),
                Some(want),
                "dropping at the {name} edge"
            );
        }
    }

    #[test]
    fn dropping_a_window_at_the_top_maximizes_it() {
        // The one edge that is not a zone. It goes through `maximize_window`,
        // so `maximized` is set and `snapped` is not -- the two are
        // alternatives, and a window that claimed both would be restored twice.
        let (mut comp, id) = with_one_window();
        drag_title_bar_to(&mut comp, id, 400, 2);
        assert!(maximized(&comp, id), "the top edge did not maximize");
        assert_eq!(snapped_to(&comp, id), None);
    }

    #[test]
    fn dropping_a_window_at_the_bottom_leaves_it_where_it_was_dropped() {
        // The bottom edge deliberately means nothing: no preset is a
        // bottom-half strip, and inventing one would make the gesture mean
        // something different from the top edge's mirror image.
        let (mut comp, id) = with_one_window();
        drag_title_bar_to(&mut comp, id, 400, 597);
        assert_eq!(snapped_to(&comp, id), None);
        assert!(!maximized(&comp, id));
        let win = comp.window_ref(id).expect("window");
        assert_eq!(
            (win.width, win.height),
            (200, 150),
            "a drop that snaps nothing must not resize anything either"
        );
    }

    #[test]
    fn a_drag_that_ends_in_open_desktop_only_moves_the_window() {
        let (mut comp, id) = with_one_window();
        drag_title_bar_to(&mut comp, id, 400, 300);
        assert_eq!(snapped_to(&comp, id), None);
        assert!(!maximized(&comp, id));
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (200, 150));
        assert_ne!(
            (win.x, win.y),
            (100, 100),
            "the drag should still have moved the window"
        );
    }

    #[test]
    fn clicking_the_title_bar_of_a_window_at_the_edge_does_not_tile_it() {
        // The gesture that makes a movement test necessary. This window's title
        // bar starts at x = 0, so a press in its leftmost columns is inside the
        // left edge band from the very first event -- and a click is a press
        // and a release at the same point with no drag between them.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        comp.set_double_click_ms(2000);
        let mut spec = WindowSpec::new("At the edge", 200, 150);
        spec.position = Some((0, 300));
        let id = comp.create_window_from_spec(&spec, 1);

        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("title bar");
        let (x, y) = (bar.x + 2, bar.y + 2);
        assert!(
            guiremote::zones::edge_at(x as f32, y as f32, comp.work_area_at(x, y)).is_some(),
            "the test's own press point is not in an edge band, so it proves nothing"
        );
        comp.handle_mouse_button(MouseButton::Left, true, x, y);
        comp.handle_mouse_button(MouseButton::Left, false, x, y);

        assert_eq!(snapped_to(&comp, id), None, "a click tiled the window");
        assert!(!maximized(&comp, id));
    }

    #[test]
    fn a_resize_drag_that_ends_at_an_edge_does_not_tile() {
        // Dragging the left border to the left edge of the screen is how a user
        // sizes a window against that edge by hand. Snapping on release would
        // overrule the size they had just finished choosing, at the one moment
        // it is too late to aim differently.
        let (mut comp, id) = with_one_window();
        let (grab_x, grab_y) = (99, 175);
        comp.handle_mouse_button(MouseButton::Left, true, grab_x, grab_y);
        assert_eq!(
            comp.drag.as_ref().map(|d| d.mode),
            Some(DragMode::ResizeLeft),
            "the test grabbed something other than the left border"
        );
        comp.handle_mouse_move(2, grab_y);
        comp.handle_mouse_button(MouseButton::Left, false, 2, grab_y);

        assert_eq!(
            snapped_to(&comp, id),
            None,
            "a resize drag tiled the window"
        );
        assert!(!maximized(&comp, id));
        let win = comp.window_ref(id).expect("window");
        // 3, not 2: the *left border* follows the pointer and the border is
        // one pixel outside the client area, which is what the grab point
        // 99 -- rather than 100 -- reflects.
        assert_eq!(win.x, 3, "the resize itself should still have happened");
    }

    #[test]
    fn an_edge_drop_lands_in_the_rectangle_the_drop_promised() {
        // The preview drawn during the drag is `EdgeDrop::rect`; the placement
        // is whatever `maximize_window` or `snap_window_to_zone` works out for
        // itself. They are separate pieces of arithmetic, so a preview that
        // lies is a real possibility -- this is what rules it out.
        let cases = [
            ("a corner", (797, 2)),
            ("an edge", (2, 300)),
            ("the top", (400, 2)),
        ];
        for (name, (x, y)) in cases {
            let (mut comp, id) = with_one_window();
            let area = comp.work_area_at(x, y);
            let promised = guiremote::zones::drop_at(x as f32, y as f32, area)
                .unwrap_or_else(|| panic!("{name} should snap"))
                .rect(area)
                .expect("resolves");
            drag_title_bar_to(&mut comp, id, x, y);
            assert_eq!(
                comp.window_ref(id).expect("window").frame_rect(),
                zone_rect(promised),
                "dropping at {name} placed the frame somewhere other than the preview"
            );
        }
    }

    #[test]
    fn a_window_the_client_declared_fixed_size_is_not_tiled_by_a_drop() {
        // Tiling is a resize, and a window that said it works at one size means
        // it. The refusal comes from `place_snapped` rather than from anything
        // here, which is the point of routing the drop through it.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        comp.set_double_click_ms(2000);
        let mut spec = WindowSpec::new("Fixed", 200, 150);
        spec.position = Some((100, 100));
        spec.resizable = false;
        let id = comp.create_window_from_spec(&spec, 1);
        drag_title_bar_to(&mut comp, id, 2, 300);
        assert_eq!(snapped_to(&comp, id), None);
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (200, 150));
    }

    #[test]
    fn a_dropped_window_remembers_the_size_it_had_before_the_drag() {
        // Routing through `place_snapped` is what earns this: the restore
        // rectangle is recorded by the same code that records it for a keyboard
        // snap, so an edge drop is undoable in exactly the way every other
        // tiling operation is.
        let (mut comp, id) = with_one_window();
        drag_title_bar_to(&mut comp, id, 2, 300);
        assert!(snapped_to(&comp, id).is_some());
        comp.restore_window(id).expect("restore");
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (200, 150));
        assert_eq!(win.snapped, None);
    }

    #[test]
    fn re_tiling_an_already_tiled_window_still_restores_to_its_own_size() {
        // Two drops in a row. The second must not record the first one's
        // half-screen geometry as the place to come back to.
        let (mut comp, id) = with_one_window();
        drag_title_bar_to(&mut comp, id, 2, 300);
        // Two title-bar presses on one window inside the double-click interval
        // are a double click, which maximizes rather than dragging. A press on
        // the desktop between them is what says the user went somewhere else
        // and came back -- the same thing that stops a real user's two slow
        // drags from being read as one fast double click.
        comp.handle_mouse_button(MouseButton::Left, true, 700, 300);
        comp.handle_mouse_button(MouseButton::Left, false, 700, 300);
        drag_title_bar_to(&mut comp, id, 797, 300);
        assert_eq!(
            snapped_to(&comp, id),
            Some(SnapTarget::Zone(slot(SnapLayoutPreset::TwoEqualHalves, 1)))
        );
        comp.restore_window(id).expect("restore");
        let win = comp.window_ref(id).expect("window");
        assert_eq!((win.width, win.height), (200, 150));
    }

    // ---- the preview drawn while the drag is still in flight ---------------
    //
    // A drop that only announces itself once the button is up asks the user to
    // aim at something invisible. The preview is the aiming aid, and because
    // it is a translucent wash over the desktop it is also the one piece of
    // this feature that can be wrong *by accumulation* rather than by
    // placement -- hence the frame-over-frame tests below alongside the
    // ordinary state ones.

    /// Press `id`'s title bar and drag the pointer to `(to_x, to_y)` **without
    /// letting go**: the drag is still running when this returns, which is the
    /// only moment a preview exists.
    fn drag_title_bar_toward(comp: &mut Compositor, id: WindowId, to_x: i32, to_y: i32) {
        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("a decorated window has a title bar");
        let from_x = bar.x + 10;
        let from_y = bar.y + bar.height as i32 / 2;
        comp.handle_mouse_button(MouseButton::Left, true, from_x, from_y);
        assert!(comp.drag.is_some(), "the title-bar press started no drag");
        comp.handle_mouse_move(to_x, to_y);
    }

    /// `with_one_window`, but with a frame budget of zero.
    ///
    /// `compose_frame` declines when it is too soon for another frame, and two
    /// synchronous calls are microseconds apart -- so at 60 Hz a test that
    /// composites twice and compares would be comparing one frame with itself
    /// and passing for it. `frame_interval_for` divides 1 MHz by the refresh
    /// rate, so any rate above that lands on a zero-length interval and every
    /// call composites.
    fn with_one_unthrottled_window() -> (Compositor, WindowId) {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.set_double_click_ms(2000);
        let mut spec = WindowSpec::new("Resizable", 200, 150);
        spec.position = Some((100, 100));
        let id = comp.create_window_from_spec(&spec, 1);
        (comp, id)
    }

    /// The rectangle the compositor is currently offering to drop into.
    fn previewed_rect(comp: &Compositor) -> Rect {
        let drop = comp.drag_preview.expect("a preview is up");
        comp.preview_rect(drop).expect("the preview drop resolves")
    }

    fn presented_pixel(comp: &Compositor, x: i32, y: i32) -> u32 {
        let (w, _) = comp.backend.size();
        comp.backend.presented_pixels()
            [Framebuffer::pixel_index(w as usize, x as usize, y as usize)]
    }

    #[test]
    fn the_four_border_bands_of_a_preview_never_overlap() {
        // The bands are painted opaquely over a translucent wash, so an
        // overlap is not merely overdraw: the shared pixels take the border
        // colour twice and come out as four darker corner squares. The sizes
        // below are chosen to include the ones where the naive symmetric
        // formula breaks -- anything shorter or narrower than twice the
        // thickness, where a top and a bottom band of full thickness would
        // meet in the middle.
        let cases = [
            (400u32, 600u32, 2u32),
            (400, 600, 0),
            (5, 5, 2),
            (4, 4, 2),
            (3, 3, 2),
            (2, 2, 2),
            (1, 1, 2),
            (0, 0, 2),
            (1, 600, 2),
            (600, 1, 2),
        ];
        for (width, height, thickness) in cases {
            let rect = Rect::new(17, 23, width, height);
            let bands = rect_outline(rect, thickness);
            for (i, a) in bands.iter().enumerate() {
                for b in bands.iter().skip(i + 1) {
                    assert_eq!(
                        a.intersect(b),
                        None,
                        "{width}x{height} at thickness {thickness}: {a:?} and {b:?} overlap"
                    );
                }
                assert!(
                    a.intersect(&rect) == Some(*a) || a.width == 0 || a.height == 0,
                    "{width}x{height} at thickness {thickness}: {a:?} reaches outside {rect:?}"
                );
            }
            // ...and together they are the ring, not some of it: the area they
            // cover is what is left after knocking the hole out of the middle.
            let painted: u32 = bands.iter().map(|b| b.width.saturating_mul(b.height)).sum();
            let side = thickness.min(width);
            let cap = thickness.min(height);
            let hole = width
                .saturating_sub(side)
                .saturating_sub(thickness.min(width.saturating_sub(side)))
                .saturating_mul(
                    height
                        .saturating_sub(cap)
                        .saturating_sub(thickness.min(height.saturating_sub(cap))),
                );
            assert_eq!(
                painted,
                width.saturating_mul(height) - hole,
                "{width}x{height} at thickness {thickness}: the bands leave a gap in the ring"
            );
        }
    }

    #[test]
    fn dragging_into_an_edge_band_raises_a_preview_and_leaving_takes_it_down() {
        // The preview has to track the pointer in both directions. One that
        // only ever appears would follow a user who changed their mind all the
        // way back into open desktop, promising a tiling that will not happen.
        let (mut comp, id) = with_one_unthrottled_window();
        drag_title_bar_toward(&mut comp, id, 400, 300);
        assert_eq!(
            comp.drag_preview, None,
            "the middle of the desktop offered to tile"
        );
        comp.handle_mouse_move(2, 300);
        assert_eq!(
            comp.drag_preview.map(|i| i.drop),
            guiremote::zones::drop_at(2.0, 300.0, comp.work_area_at(2, 300)),
            "the left edge raised something other than the left edge's drop"
        );
        comp.handle_mouse_move(400, 300);
        assert_eq!(
            comp.drag_preview, None,
            "the pointer left the band and the preview stayed"
        );
    }

    #[test]
    fn the_preview_promises_the_rectangle_the_drop_delivers() {
        // Stronger than comparing the drop's rectangle against the placement:
        // this reads the rectangle out of the live preview state, so a preview
        // that resolves its drop against a different work area than the drop
        // does -- the two calls are in different methods -- is visible here and
        // nowhere else.
        let cases = [
            ("a corner", (797, 2)),
            ("an edge", (2, 300)),
            ("the top", (400, 2)),
        ];
        for (name, (x, y)) in cases {
            let (mut comp, id) = with_one_unthrottled_window();
            drag_title_bar_toward(&mut comp, id, x, y);
            assert!(comp.drag_preview.is_some(), "{name} raised no preview");
            let promised = previewed_rect(&comp);
            comp.handle_mouse_button(MouseButton::Left, false, x, y);
            assert_eq!(
                comp.window_ref(id).expect("window").frame_rect(),
                promised,
                "the preview at {name} showed one rectangle and the drop used another"
            );
        }
    }

    #[test]
    fn letting_go_takes_the_preview_down() {
        let (mut comp, id) = with_one_unthrottled_window();
        drag_title_bar_toward(&mut comp, id, 2, 300);
        assert!(comp.drag_preview.is_some(), "no preview to take down");
        comp.handle_mouse_button(MouseButton::Left, false, 2, 300);
        assert_eq!(
            comp.drag_preview, None,
            "the preview outlived the drag it belonged to"
        );
    }

    #[test]
    fn closing_the_dragged_window_takes_the_preview_down() {
        // A client may destroy its window at any moment, including in the
        // middle of a drag of it. Without this the compositor would go on
        // offering to tile a window that no longer exists, and the offer would
        // never come down because the release that would cancel it belongs to
        // a drag whose window is gone.
        let (mut comp, id) = with_one_unthrottled_window();
        drag_title_bar_toward(&mut comp, id, 2, 300);
        assert!(comp.drag_preview.is_some(), "no preview to take down");
        comp.destroy_window(id).expect("destroy");
        assert!(
            comp.drag.is_none(),
            "a drag went on running against a destroyed window"
        );
        assert_eq!(
            comp.drag_preview, None,
            "the desktop is still offering to tile a window that is gone"
        );
    }

    #[test]
    fn a_resize_drag_raises_no_preview() {
        // The mirror of `a_resize_drag_that_ends_at_an_edge_does_not_tile`, at
        // the other end of the gesture: a preview that appeared during a resize
        // would be a promise the release then refuses to keep.
        let (mut comp, _id) = with_one_unthrottled_window();
        comp.handle_mouse_button(MouseButton::Left, true, 99, 175);
        assert_eq!(
            comp.drag.as_ref().map(|d| d.mode),
            Some(DragMode::ResizeLeft),
            "the test grabbed something other than the left border"
        );
        comp.handle_mouse_move(2, 175);
        assert_eq!(
            comp.drag_preview, None,
            "sizing a window against the left edge offered to tile it"
        );
    }

    #[test]
    fn the_preview_paints_its_own_rectangle_and_nothing_outside_it() {
        // Two composites of the same scene, one with the preview up and one
        // without. Every pixel outside the preview's rectangle has to match,
        // and at least one inside it has to differ -- the second half is what
        // stops the first from passing on a preview that draws nothing at all.
        //
        // The comparison is deliberately not "every pixel inside differs":
        // the preview is a wash of `border_focused` and the focused window's
        // own border is already that colour, so the pixels where it lands on
        // the border are entitled to come out unchanged.
        let scene = |preview: bool| {
            let (mut comp, _) = with_one_unthrottled_window();
            if preview {
                let area = comp.work_area_at(2, 300);
                let drop = guiremote::zones::drop_at(2.0, 300.0, area);
                comp.set_drag_preview(drop.map(|drop| DropIntent { drop, area }));
            }
            assert!(comp.compose_frame(), "the frame was refused");
            comp.backend.presented_pixels().to_vec()
        };
        let (with, without) = (scene(true), scene(false));

        let area = Compositor::new(800, 600, 60)
            .expect("compositor")
            .work_area_at(2, 300);
        let rect = zone_rect(
            guiremote::zones::drop_at(2.0, 300.0, area)
                .expect("the left edge snaps")
                .rect(area)
                .expect("resolves"),
        );
        let mut differed_inside = 0u32;
        for (i, (a, b)) in without.iter().zip(&with).enumerate() {
            let (x, y) = ((i % 800) as i32, (i / 800) as i32);
            if rect.contains(x, y) {
                differed_inside += u32::from(a != b);
            } else {
                assert_eq!(
                    a, b,
                    "the preview repainted ({x}, {y}), which is outside its own rectangle"
                );
            }
        }
        assert!(
            differed_inside > 0,
            "the preview changed nothing inside its own rectangle -- it was not drawn"
        );
    }

    #[test]
    fn a_preview_held_in_one_place_does_not_deepen_frame_by_frame() {
        // The preview is a translucent wash. Painted over a *partial* frame it
        // blends onto pixels that already carry the previous frame's copy of
        // itself, so it darkens a little every frame until it is opaque -- and
        // a drag rests in one edge band for hundreds of frames, which is
        // exactly long enough for a user to watch it happen. The fix is to
        // composite whole whenever a preview is up.
        //
        // The pointer moves a pixel between frames so the frames are not
        // trivially identical: without that there is no partial frame for the
        // wash to compound over and the test could not fail.
        let (mut comp, id) = with_one_unthrottled_window();
        drag_title_bar_toward(&mut comp, id, 2, 300);
        let rect = previewed_rect(&comp);
        // Inside the preview and clear of the dragged window, so nothing but
        // the preview itself can be responsible for the pixel.
        let (px, py) = (rect.x + rect.width as i32 / 2, rect.bottom() - 4);
        assert!(
            !comp
                .window_ref(id)
                .expect("window")
                .frame_rect()
                .contains(px, py),
            "the probe pixel is under the dragged window, so it proves nothing"
        );
        assert!(comp.compose_frame(), "the first frame was refused");
        let first = presented_pixel(&comp, px, py);
        for step in 1..=4 {
            comp.handle_mouse_move(2 + step, 300);
            assert_eq!(previewed_rect(&comp), rect, "the preview moved");
            assert!(comp.compose_frame(), "frame {step} was refused");
            assert_eq!(
                presented_pixel(&comp, px, py),
                first,
                "the preview is a different colour on frame {step} than on the first, \
                 which is it blending over its own previous copy"
            );
        }
    }

    #[test]
    fn taking_the_preview_down_damages_the_ground_it_covered() {
        // The frame that *has* a preview is composited whole, but the frame
        // after it is not -- so the rectangle the preview vacated is repainted
        // only if something marks it dirty, and nothing else will: no window
        // moved there and the desktop under it never changed. Without this the
        // preview stays burned into the screen after the pointer leaves the
        // band, until something unrelated happens to repaint that half.
        let (mut comp, id) = with_one_unthrottled_window();
        drag_title_bar_toward(&mut comp, id, 2, 300);
        let covered = previewed_rect(&comp);
        assert!(comp.compose_frame(), "the preview frame was refused");
        assert!(
            !comp.damage.has_damage(),
            "compositing left damage behind, so the check below would pass on it"
        );
        comp.handle_mouse_move(400, 300);
        assert_eq!(comp.drag_preview, None, "the preview did not come down");
        assert!(
            comp.damage
                .rects()
                .iter()
                .any(|r| r.intersect(&covered) == Some(covered)),
            "the preview came down without marking the {covered:?} it had covered, \
             and the frame that takes it down is not composited whole"
        );
    }

    // ---- tiling divides one monitor, not the whole virtual desktop ----

    /// Two side-by-side monitors, with a window on whichever one is asked for.
    ///
    /// 800x600 primary and a 1024x768 secondary, which
    /// [`DisplayManager::add_display`] places immediately to its right — so the
    /// seam is at x = 800 and the virtual desktop is 1824 wide, a width no
    /// single monitor has. The window is placed 100px inside the monitor named,
    /// well clear of the seam, so `display_for`'s largest-overlap rule cannot
    /// answer the other one by accident.
    ///
    /// Returns both monitors' bounds, because every assertion below is of the
    /// form "this landed on that monitor and not on the union of both".
    fn two_monitors(window_on: usize) -> (Compositor, WindowId, [Rect; 2]) {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.attach_display(Display::new(1, 1024, 768, 60, 1.0, false))
            .expect("attach");
        let screens = [
            comp.display_manager.displays()[0].bounds(),
            comp.display_manager.displays()[1].bounds(),
        ];
        let home = screens[window_on];
        let mut spec = WindowSpec::new("Over there", 200, 150);
        spec.position = Some((home.x + 100, home.y + 100));
        let id = comp.create_window_from_spec(&spec, 1);
        (comp, id, screens)
    }

    #[test]
    fn maximizing_fills_the_windows_own_monitor_and_not_every_monitor() {
        // The bug this pins: `maximize_window` measured the *union* of every
        // display, so a window on the second monitor was made 1824px wide and
        // moved to x=0 -- onto the first monitor, spanning both. A one-monitor
        // desktop cannot tell that reading from the right one, which is why
        // every test had one.
        let (mut comp, id, screens) = two_monitors(1);
        comp.maximize_window(id).expect("maximize");
        let got = comp.window_ref(id).expect("window").frame_rect();
        assert_eq!(
            (got.x, got.width),
            (screens[1].x, screens[1].width),
            "maximize did not fill the window's own monitor"
        );
        assert!(
            got.intersect(&screens[0]).is_none(),
            "the maximized window spilled onto the other monitor"
        );
    }

    #[test]
    fn snapping_takes_half_of_the_windows_own_monitor() {
        for (edge, want_x) in [(SnapEdge::Left, 0), (SnapEdge::Right, 512)] {
            let (mut comp, id, screens) = two_monitors(1);
            comp.snap_window(id, edge).expect("snap");
            let got = comp.window_ref(id).expect("window").frame_rect();
            assert_eq!(
                (got.x, got.width),
                (screens[1].x + want_x, screens[1].width / 2),
                "{edge:?} did not take half of the window's own monitor"
            );
        }
    }

    #[test]
    fn a_zone_snap_resolves_against_the_windows_own_monitor() {
        // The leftmost zone of the three-column layout: a third of one monitor,
        // not a third of the desktop.
        let (mut comp, id, screens) = two_monitors(1);
        let target = slot(SnapLayoutPreset::ThreeColumns, 0);
        comp.snap_window_to_zone(id, target).expect("snap to zone");
        let want = zone_rect(
            target
                .rect(work_area_of(screens[1]))
                .expect("the preset has a zone 0"),
        );
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            want,
            "the zone was resolved against something other than the window's monitor"
        );
    }

    #[test]
    fn an_edge_drop_uses_the_monitor_the_pointer_is_over() {
        // Drag a window that lives on the *first* monitor to the *second*
        // monitor's left edge. At the moment of release the window is still
        // mostly on the first -- so a release that re-derived the work area
        // from the window would tile it on the screen it was dragged away
        // from, after showing an outline on the screen it was dragged to.
        let (mut comp, id, screens) = two_monitors(0);
        let aim = (screens[1].x + 2, screens[1].y + 300);
        drag_title_bar_to(&mut comp, id, aim.0, aim.1);
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            edge_drop_rect(aim.0, aim.1, screens[1]),
            "the drop tiled against a monitor other than the one aimed at"
        );
    }

    /// Where a drop at `(x, y)` belongs if `screen` is the monitor it is aimed
    /// at — computed from `guiremote::zones` rather than by halving the width,
    /// because a zone rectangle carries the layout's gap and a hand-computed
    /// half does not.
    #[allow(
        clippy::cast_precision_loss,
        reason = "test coordinates are far inside f32's exact-integer range"
    )]
    fn edge_drop_rect(x: i32, y: i32, screen: Rect) -> Rect {
        let area = work_area_of(screen);
        match guiremote::zones::drop_at(x as f32, y as f32, area)
            .expect("the aim point is in an edge band")
        {
            EdgeDrop::Maximize => screen,
            EdgeDrop::Zone(slot) => zone_rect(slot.rect(area).expect("the slot resolves")),
        }
    }

    #[test]
    fn a_drop_tiles_the_monitor_the_pointer_is_over_even_when_the_window_is_not() {
        // The case the tests above cannot reach. A move drag carries the window
        // along under the pointer, so for an ordinary grab near the *left* end
        // of the title bar the window and the pointer are on the same monitor
        // by the time the pointer reaches an edge band, and "which of the two
        // decides the monitor" makes no difference.
        //
        // Grab a wide window a long way along its title bar and the two come
        // apart: the window trails 600px behind the pointer, so when the
        // pointer is 100px into the second monitor the window is still mostly
        // on the first. That is the only configuration in which a release that
        // re-derived the work area from the window tiles a different screen
        // from the one whose outline the user was watching.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.attach_display(Display::new(1, 1024, 768, 60, 1.0, false))
            .expect("attach");
        let screens = [
            comp.display_manager.displays()[0].bounds(),
            comp.display_manager.displays()[1].bounds(),
        ];
        let mut spec = WindowSpec::new("Wide", 900, 150);
        spec.position = Some((20, 100));
        let id = comp.create_window_from_spec(&spec, 1);

        let bar = comp
            .window_ref(id)
            .expect("window")
            .title_bar_rect()
            .expect("a decorated window has a title bar");
        // Two thirds along, well clear of the buttons at the right-hand end.
        let grab = bar.x + (bar.width as i32 * 2) / 3;
        let grab_y = bar.y + bar.height as i32 / 2;
        comp.handle_mouse_button(MouseButton::Left, true, grab, grab_y);
        assert!(comp.drag.is_some(), "the title-bar press started no drag");

        // The top band of the second monitor, clear of both its corners, so
        // the drop means Maximize rather than a quarter zone.
        let (aim_x, aim_y) = (screens[1].x + 100, screens[1].y + 2);
        comp.handle_mouse_move(aim_x, aim_y);

        let dragged = comp.window_ref(id).expect("window").frame_rect();
        let on_first = dragged.intersect(&screens[0]).map_or(0, |r| r.width);
        let on_second = dragged.intersect(&screens[1]).map_or(0, |r| r.width);
        assert!(
            on_first > on_second,
            "the fixture failed to separate the two: the window is already \
             mostly on the monitor the pointer is over ({on_first} vs {on_second})"
        );
        assert_eq!(
            previewed_rect(&comp),
            screens[1],
            "the outline promised something other than the pointer's monitor"
        );

        comp.handle_mouse_button(MouseButton::Left, false, aim_x, aim_y);
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            screens[1],
            "the drop filled a different monitor from the one the outline promised"
        );
    }

    #[test]
    fn the_preview_crosses_the_seam_with_the_pointer() {
        // The other half of the test above: what the user is *shown* while the
        // pointer is over the second monitor. Two aims that are the same edge
        // of two different screens, so a preview computed against the union
        // would answer the same rectangle for both.
        let (mut comp, id, screens) = two_monitors(0);
        drag_title_bar_toward(&mut comp, id, screens[1].x + 2, 300);
        let on_second = previewed_rect(&comp);
        comp.handle_mouse_move(screens[0].x + 2, 300);
        let on_first = previewed_rect(&comp);
        assert_eq!(
            on_second,
            edge_drop_rect(screens[1].x + 2, 300, screens[1]),
            "the preview on the second monitor was not that monitor's left half"
        );
        assert_eq!(
            on_first,
            edge_drop_rect(screens[0].x + 2, 300, screens[0]),
            "the preview on the first monitor was not that monitor's left half"
        );
        assert_ne!(
            on_first, on_second,
            "the two monitors' left halves came out as the same rectangle"
        );
    }

    #[test]
    fn the_interior_seam_is_two_edges_and_not_a_middle() {
        // Between two monitors there is no "middle of the desktop": the last
        // column of the left screen is that screen's *right* edge and the first
        // column of the right screen is that screen's *left* edge. Against the
        // union both points were interior and neither tiled anything.
        // A fresh compositor per case rather than a restore between them: two
        // synchronous drags of the same title bar fall inside the double-click
        // interval, so the second press would be read as a double-click and
        // maximize the window instead of starting a drag.
        let seam = two_monitors(0).2;
        for (name, at, screen) in [
            ("the left screen's right edge", seam[0].right() - 2, 0),
            ("the right screen's left edge", seam[1].x + 2, 1),
        ] {
            let (mut comp, id, screens) = two_monitors(0);
            drag_title_bar_to(&mut comp, id, at, 300);
            let want = edge_drop_rect(at, 300, screens[screen]);
            assert_eq!(
                comp.window_ref(id).expect("window").frame_rect(),
                want,
                "a drop at {name} did not tile against it"
            );
            assert!(
                want.intersect(&screens[1 - screen]).is_none(),
                "a drop at {name} reached across the seam"
            );
        }
    }

    #[test]
    fn a_work_area_survives_the_round_trip_back_to_pixels() {
        // `work_rect` is the inverse of `work_area_of`, and the drop path
        // relies on it: the release maximizes into `work_rect(intent.area)`,
        // so a lossy conversion would put a maximized window a pixel off the
        // rectangle its own preview promised.
        for bounds in [
            Rect::new(0, 0, 800, 600),
            Rect::new(800, 0, 1024, 768),
            Rect::new(-1920, -120, 1920, 1080),
            Rect::new(0, 0, 0, 0),
        ] {
            assert_eq!(
                work_rect(work_area_of(bounds)),
                bounds,
                "the work area of {bounds:?} did not come back as itself"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Edge reservations -- TD-C-COMPOSITOR-TILES-UNDER-THE-TASKBAR
    // -----------------------------------------------------------------------

    /// A panel window sitting in the bottom `height` pixels of `screen`.
    ///
    /// Placed where a real taskbar would be rather than anywhere on the
    /// monitor, because `reserved_on` finds it by asking which monitor it
    /// overlaps most: a panel parked in the corner of the wrong screen would
    /// reserve out of that one, and a test that placed it carelessly would pass
    /// for the wrong reason.
    fn add_panel(comp: &mut Compositor, screen: Rect, height: u32) -> WindowId {
        let mut spec = WindowSpec::new("Taskbar", screen.width, height);
        spec.decorations = false;
        spec.position = Some((
            screen.x,
            screen.y + i32::try_from(screen.height.saturating_sub(height)).unwrap_or(0),
        ));
        comp.create_window_from_spec(&spec, 99)
    }

    #[test]
    fn a_maximized_window_stops_at_the_taskbar_instead_of_going_under_it() {
        // The bug: every tiling path divided the *whole* monitor, so the bottom
        // 40 rows of a maximized window were behind the bar. Nothing in the
        // compositor knew a panel existed to leave room for.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);

        let mut spec = WindowSpec::new("App", 200, 150);
        spec.position = Some((50, 50));
        let app = comp.create_window_from_spec(&spec, 1);

        comp.maximize_window(app).expect("maximize");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect().bottom(),
            screen.bottom(),
            "without a reservation, maximizing should still reach the screen edge"
        );

        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        comp.maximize_window(app).expect("maximize again");
        let framed = comp.window_ref(app).expect("window").frame_rect();
        assert_eq!(
            framed.bottom(),
            screen.bottom() - 40,
            "the maximized window still reaches under the taskbar"
        );
        assert_eq!(framed.y, screen.y, "the top of the screen was lost too");
        assert_eq!(framed.width, screen.width, "the width changed as well");
    }

    #[test]
    fn a_reservation_answers_with_the_area_it_left() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        let area = comp
            .reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        assert_eq!(
            work_rect(area),
            Rect::new(screen.x, screen.y, screen.width, screen.height - 40),
            "the reply did not describe the area the reservation actually left"
        );
    }

    #[test]
    fn releasing_a_reservation_gives_the_strip_back() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);

        let mut spec = WindowSpec::new("App", 200, 150);
        spec.position = Some((50, 50));
        let app = comp.create_window_from_spec(&spec, 1);

        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        comp.maximize_window(app).expect("maximize");
        // Zero is the release: there is deliberately no second request for it,
        // so that a panel has one code path to "how much do I need" rather than
        // two, and the compositor one place to re-tile from.
        let area = comp
            .reserve_edge(panel, PanelEdge::Bottom, 0)
            .expect("release");
        assert_eq!(
            work_rect(area),
            screen,
            "releasing did not give the whole monitor back"
        );
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect().bottom(),
            screen.bottom(),
            "the already-maximized window was not re-grown into the freed strip"
        );
    }

    #[test]
    fn a_second_reservation_from_one_panel_replaces_rather_than_adds() {
        // A taskbar that grew from 40 to 56 sends 56, not 16. If the two added,
        // a panel that changed height a few times would eat the desktop.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        let area = comp
            .reserve_edge(panel, PanelEdge::Bottom, 56)
            .expect("re-reserve");
        assert_eq!(
            work_rect(area).height,
            screen.height - 56,
            "two reservations from one panel added up instead of replacing"
        );
    }

    #[test]
    fn two_panels_on_one_monitor_both_get_their_strip() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let bar = add_panel(&mut comp, screen, 40);

        let mut spec = WindowSpec::new("Menu bar", screen.width, 24);
        spec.decorations = false;
        spec.position = Some((screen.x, screen.y));
        let menu = comp.create_window_from_spec(&spec, 98);

        comp.reserve_edge(bar, PanelEdge::Bottom, 40).expect("bar");
        let area = comp.reserve_edge(menu, PanelEdge::Top, 24).expect("menu");
        assert_eq!(
            work_rect(area),
            Rect::new(screen.x, screen.y + 24, screen.width, screen.height - 64),
            "the two panels did not each get their own strip"
        );
    }

    #[test]
    fn a_panel_reserves_only_out_of_its_own_monitor() {
        // The multi-monitor half of the same question the union-of-displays bug
        // asked: a taskbar on the second screen must not shrink tiling on the
        // first.
        let (mut comp, app, screens) = two_monitors(0);
        let panel = add_panel(&mut comp, screens[1], 40);

        // Maximized *before* the reservation as well as after, because the two
        // ask different questions. Before: does the re-tile leave alone the
        // windows on the monitor that did not change? After: does a fresh
        // tiling on the first monitor measure the first monitor?
        comp.maximize_window(app).expect("maximize");
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect(),
            screens[0],
            "the second monitor's panel re-tiled a window on the first"
        );

        comp.maximize_window(app).expect("maximize again");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect(),
            screens[0],
            "a panel on the second monitor shrank tiling on the first"
        );
        assert_eq!(
            work_rect(comp.work_area_for(screens[1])).height,
            screens[1].height - 40,
            "the panel did not reserve out of its own monitor either"
        );
    }

    #[test]
    fn a_hidden_panel_reserves_nothing() {
        // A strip is kept clear so what sits in it stays visible. Nothing is
        // sitting in it while the panel is hidden, so holding the strip would
        // shrink the desktop with nothing on screen to explain why.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        assert_eq!(
            work_rect(comp.work_area_for(screen)).height,
            screen.height - 40
        );

        comp.set_visible(panel, false).expect("hide");
        assert_eq!(
            work_rect(comp.work_area_for(screen)),
            screen,
            "a hidden panel kept its strip"
        );
        comp.set_visible(panel, true).expect("show");
        assert_eq!(
            work_rect(comp.work_area_for(screen)).height,
            screen.height - 40,
            "showing the panel again did not take its strip back"
        );
    }

    #[test]
    fn destroying_a_panel_releases_its_reservation() {
        // The reason the claim lives on the window rather than in a table: a
        // panel that crashes must not carve a permanent strip out of the
        // desktop with nothing left on screen to release it.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        comp.destroy_window(panel).expect("destroy");
        assert_eq!(
            work_rect(comp.work_area_for(screen)),
            screen,
            "a destroyed panel's strip outlived it"
        );
    }

    #[test]
    fn a_greedy_reservation_is_clamped_rather_than_erasing_the_desktop() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        let area = comp
            .reserve_edge(panel, PanelEdge::Bottom, 100_000)
            .expect("reserve");
        assert!(
            area.height > 0.0,
            "a client asking for the whole screen left no work area at all"
        );
        let kept = work_area_of(screen).height * (1.0 - guiremote::reserve::MAX_RESERVED_FRACTION);
        assert!(
            (area.height - kept).abs() < 1.0,
            "the clamp left {} of {}, not two thirds",
            area.height,
            screen.height
        );
    }

    #[test]
    fn every_tiling_route_respects_the_reservation_and_not_just_maximize() {
        // The point of putting the subtraction in `work_area_for`: the half
        // snap, each zone slot and the edge drop all resolve from a `WorkArea`,
        // so one place fixes all of them. Walking each route is what proves the
        // funnel is real rather than assumed.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.set_double_click_ms(2000);
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        let usable = screen.bottom() - 40;

        let mut spec = WindowSpec::new("App", 200, 150);
        spec.position = Some((50, 50));
        let app = comp.create_window_from_spec(&spec, 1);

        comp.snap_window(app, SnapEdge::Left).expect("snap left");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect().bottom(),
            usable,
            "a half snap still reached under the taskbar"
        );

        let zone = slot(SnapLayoutPreset::TwoEqualHalves, 1);
        comp.snap_window_to_zone(app, zone).expect("snap to zone");
        assert!(
            comp.window_ref(app).expect("window").frame_rect().bottom() <= usable,
            "a zone snap still reached under the taskbar"
        );

        // And the drop preview, which is drawn from the same area.
        comp.restore_window(app).expect("restore");
        let bar = comp
            .window_ref(app)
            .expect("window")
            .title_bar_rect()
            .expect("the app is decorated");
        comp.handle_mouse_button(MouseButton::Left, true, bar.x + 10, bar.y + 5);
        comp.handle_mouse_move(2, 300);
        let previewed = previewed_rect(&comp);
        assert!(
            previewed.bottom() <= usable,
            "the edge-drop preview promised {previewed:?}, which reaches under the taskbar"
        );
        comp.handle_mouse_button(MouseButton::Left, false, 2, 300);
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect(),
            previewed,
            "the drop did not land where the preview promised"
        );
    }

    #[test]
    fn a_taskbar_appearing_re_tiles_the_windows_that_are_already_tiled() {
        // A tiled window holds a *rectangle*, not the rule that produced it, so
        // a panel appearing after the tiling leaves every such window exactly
        // where it was -- underneath the bar, which is the bug the reservation
        // exists to prevent.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);

        let mut spec = WindowSpec::new("Maxed", 200, 150);
        spec.position = Some((50, 50));
        let maxed = comp.create_window_from_spec(&spec, 1);
        let mut spec = WindowSpec::new("Halved", 200, 150);
        spec.position = Some((60, 60));
        let halved = comp.create_window_from_spec(&spec, 1);
        let mut spec = WindowSpec::new("Zoned", 200, 150);
        spec.position = Some((70, 70));
        let zoned = comp.create_window_from_spec(&spec, 1);
        let mut spec = WindowSpec::new("Loose", 200, 150);
        spec.position = Some((80, 400));
        let loose = comp.create_window_from_spec(&spec, 1);

        comp.maximize_window(maxed).expect("maximize");
        comp.snap_window(halved, SnapEdge::Left).expect("snap");
        comp.snap_window_to_zone(zoned, slot(SnapLayoutPreset::TwoEqualHalves, 1))
            .expect("zone");
        let loose_before = comp.window_ref(loose).expect("window").frame_rect();

        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");

        let usable = screen.bottom() - 40;
        for (id, what) in [
            (maxed, "maximized"),
            (halved, "half-snapped"),
            (zoned, "zoned"),
        ] {
            assert!(
                comp.window_ref(id).expect("window").frame_rect().bottom() <= usable,
                "the already-{what} window was not re-placed and still reaches under the bar"
            );
        }
        assert_eq!(
            comp.window_ref(loose).expect("window").frame_rect(),
            loose_before,
            "an untiled window was moved by a reservation it has nothing to do with"
        );
    }

    #[test]
    fn re_tiling_for_a_reservation_keeps_the_restore_rectangle() {
        // The re-place goes back through `maximize_window_within`, which records
        // `restore_rect` only for a window that was not already tiled. If that
        // guard were missed, a taskbar appearing would overwrite every tiled
        // window's memory of where it came from with the tile it is sitting in.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);

        let mut spec = WindowSpec::new("App", 200, 150);
        spec.position = Some((50, 50));
        let app = comp.create_window_from_spec(&spec, 1);
        let home = comp.window_ref(app).expect("window").frame_rect();

        comp.maximize_window(app).expect("maximize");
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        comp.restore_window(app).expect("restore");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect(),
            home,
            "the reservation's re-tile overwrote where the window came from"
        );
    }

    #[test]
    fn reserving_against_a_window_that_is_gone_is_an_error_and_not_a_panic() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.destroy_window(panel).expect("destroy");
        assert!(
            comp.reserve_edge(panel, PanelEdge::Bottom, 40).is_err(),
            "reserving against a destroyed window succeeded"
        );
    }

    // -----------------------------------------------------------------------
    // Display resolution changes -- TD-C-COMPOSITOR-CANNOT-CHANGE-MODE
    // -----------------------------------------------------------------------

    /// An ordinary decorated application window at a chosen place and size.
    fn app_at(comp: &mut Compositor, x: i32, y: i32, width: u32, height: u32) -> WindowId {
        let mut spec = WindowSpec::new("App", width, height);
        spec.position = Some((x, y));
        comp.create_window_from_spec(&spec, 1)
    }

    #[test]
    fn a_maximised_window_follows_the_display_to_its_new_size() {
        // A tiled window holds the *rectangle* its rule produced and not the
        // rule, so a mode switch left it exactly where it was. On a screen that
        // shrank, the right-hand third of the window was in pixels that no
        // longer existed; on one that grew, a band of bare desktop ran down two
        // sides of a window still claiming to be maximised.
        for &(width, height) in &[(1280_u32, 1024_u32), (2560, 1440)] {
            let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
            let id = app_at(&mut comp, 40, 40, 200, 150);
            comp.maximize_window(id).expect("maximize");
            comp.resize_display(width, height).expect("resize");
            assert_eq!(
                comp.window_ref(id).expect("window").frame_rect(),
                Rect::new(0, 0, width, height),
                "a maximised window did not follow the display to {width}x{height}"
            );
        }
    }

    #[test]
    fn a_half_snapped_window_follows_the_display_to_its_new_size() {
        // Maximise and snap are separate rules with separate stored rectangles,
        // so proving one followed says nothing about the other.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 40, 40, 200, 150);
        comp.snap_window(id, SnapEdge::Left).expect("snap");
        comp.resize_display(1280, 1024).expect("resize");
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            Rect::new(0, 0, 640, 1024),
            "a left-snapped window kept half of the *old* screen"
        );
    }

    #[test]
    fn a_resize_re_tiles_around_the_taskbar_rather_than_over_it() {
        // The re-tile has to go through `work_area_for`, not straight to the
        // display bounds: a maximised window that filled the new screen exactly
        // would be a window drawn underneath the taskbar, which is the bug
        // reservations exist to prevent.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        let app = app_at(&mut comp, 40, 40, 200, 150);
        comp.maximize_window(app).expect("maximize");

        comp.resize_display(1280, 1024).expect("resize");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect(),
            Rect::new(0, 0, 1280, 1024 - 40),
            "the re-tile ignored the strip the taskbar had reserved"
        );
    }

    #[test]
    fn a_fullscreen_window_follows_the_display_to_its_new_size() {
        // `set_fullscreen` sizes the window from the framebuffer, so a
        // fullscreen game kept the *old* framebuffer's dimensions across a mode
        // switch: letterboxed on a screen that grew, and spilling off the
        // bottom-right on one that shrank.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 40, 40, 200, 150);
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.pending_notifications.clear();

        comp.resize_display(1280, 1024).expect("resize");
        assert_eq!(
            comp.window_ref(id).expect("window").client_rect(),
            Rect::new(0, 0, 1280, 1024),
            "a fullscreen window kept the old display's size"
        );
        assert!(
            comp.pending_notifications.iter().any(|n| matches!(
                n,
                EventNotification::WindowResized {
                    window_id,
                    width: 1280,
                    height: 1024,
                } if *window_id == id
            )),
            "the client was never told its fullscreen surface had changed size"
        );
    }

    /// A window that is fullscreen *and* still carries the `maximized` state
    /// underneath it, which is what `set_fullscreen` leaves behind for anything
    /// maximised first.
    fn fullscreen_game_over_a_taskbar(comp: &mut Compositor) -> WindowId {
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        let game = app_at(comp, 40, 40, 200, 150);
        comp.maximize_window(game).expect("maximize");
        comp.set_fullscreen(game, true).expect("fullscreen");
        game
    }

    #[test]
    fn a_taskbar_appearing_does_not_shrink_a_fullscreen_window() {
        // The re-tile hunts for `maximized || snapped`, and a fullscreen window
        // maximised beforehand matches. Handing it the taskbar's leftovers
        // shrinks a game away from the screen edges with nothing on this path
        // to re-assert fullscreen afterwards, and silently disqualifies it from
        // the direct-scanout bypass, which needs a display-sized surface.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let game = fullscreen_game_over_a_taskbar(&mut comp);
        let full = comp.window_ref(game).expect("window").client_rect();

        let second = add_panel(&mut comp, Rect::new(0, 0, 1920, 1080), 60);
        comp.reserve_edge(second, PanelEdge::Top, 60)
            .expect("reserve");
        assert_eq!(
            comp.window_ref(game).expect("window").client_rect(),
            full,
            "a panel's reservation took the screen away from a fullscreen window"
        );
    }

    #[test]
    fn a_fullscreen_window_outranks_the_maximised_state_underneath_it() {
        // Same collision, reached through the resize instead: whatever order
        // the re-tile and the re-fit run in, the answer for a window that is
        // both has to be the whole new screen and not the work area.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let game = fullscreen_game_over_a_taskbar(&mut comp);

        comp.resize_display(1280, 1024).expect("resize");
        assert_eq!(
            comp.window_ref(game).expect("window").client_rect(),
            Rect::new(0, 0, 1280, 1024),
            "the re-tile took the whole screen away from a fullscreen window"
        );
    }

    #[test]
    fn a_window_stranded_off_the_new_screen_is_brought_back() {
        // The window is not merely inconvenient to reach: with no pixel of its
        // title bar on the screen there is nothing left to drag it back by, so
        // without this it is lost until the display is made large again.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 1500, 900, 300, 150);
        comp.resize_display(800, 600).expect("resize");

        let frame = comp.window_ref(id).expect("window").frame_rect();
        assert!(
            frame.intersect(&Rect::new(0, 0, 800, 600)).is_some(),
            "a window stranded by the shrink was left off the screen at {frame:?}"
        );
        assert_eq!(
            (frame.x, frame.y),
            (
                800 - i32::try_from(frame.width).expect("frame width"),
                600 - i32::try_from(frame.height).expect("frame height"),
            ),
            "the rescue moved the window further than it had to"
        );
    }

    #[test]
    fn a_window_still_on_the_new_screen_is_left_exactly_where_it_was() {
        // The rescue must be a rescue and not a re-layout: a window the user
        // can still see and still grab keeps the position they put it in, even
        // if part of it now hangs off the edge.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let near = app_at(&mut comp, 10, 10, 300, 150);
        let straddling = app_at(&mut comp, 700, 500, 300, 150);
        let before = (
            comp.window_ref(near).expect("window").frame_rect(),
            comp.window_ref(straddling).expect("window").frame_rect(),
        );

        comp.resize_display(800, 600).expect("resize");
        assert_eq!(
            (
                comp.window_ref(near).expect("window").frame_rect(),
                comp.window_ref(straddling).expect("window").frame_rect(),
            ),
            before,
            "the resize moved a window that was still reachable"
        );
    }

    #[test]
    fn a_window_larger_than_the_new_screen_keeps_its_title_bar_reachable() {
        // Pulling the frame fully inside is impossible here, so the clamp has
        // to prefer the top-left: a bottom-right anchor would put the title bar
        // above the top edge and leave the window as unreachable as it started.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 1500, 900, 1200, 900);
        comp.resize_display(800, 600).expect("resize");

        let win = comp.window_ref(id).expect("window");
        let bar = win.title_bar_rect().expect("decorated");
        assert_eq!(
            (win.frame_rect().x, win.frame_rect().y),
            (0, 0),
            "an oversized window was not pinned to the top-left corner"
        );
        assert!(
            bar.intersect(&Rect::new(0, 0, 800, 600))
                .is_some_and(|seen| seen.height == bar.height),
            "the whole title bar has to be on screen to be grabbed"
        );
    }

    #[test]
    fn the_pointer_is_brought_inside_a_display_that_shrank() {
        // The cursor is drawn at this position and every hit test starts from
        // it, so a pointer left at a coordinate the screen no longer has is an
        // invisible pointer that reports hovering over nothing.
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        comp.handle_input(InputEvent::MouseMove { x: 1900, y: 1000 });
        comp.resize_display(800, 600).expect("resize");
        assert_eq!(
            comp.cursor_position(),
            (799, 599),
            "the pointer was left outside the new display"
        );
    }

    #[test]
    fn a_pointer_already_on_the_new_screen_is_not_moved() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        comp.handle_input(InputEvent::MouseMove { x: 100, y: 200 });
        comp.resize_display(800, 600).expect("resize");
        assert_eq!(comp.cursor_position(), (100, 200), "the pointer was moved");
    }

    // -----------------------------------------------------------------------
    // Reachability: a window is never placed where it cannot be reached
    // -----------------------------------------------------------------------

    #[test]
    fn resizing_one_monitor_does_not_evacuate_the_other() {
        // `two_monitors(1)` puts an 800x600 primary at the origin, a 1024x768
        // second screen flush to its right, and the window comfortably inside
        // the second one -- nowhere near the first.
        let (mut comp, id, _) = two_monitors(1);
        let before = comp.window_ref(id).expect("window").frame_rect();

        comp.resize_display(400, 300).expect("resize");

        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "a window on the second monitor was dragged onto the first \
             because the first was resized"
        );
    }

    #[test]
    fn un_maximising_after_a_shrink_leaves_the_window_reachable() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        // Near the old bottom-right corner, so the saved rectangle is off the
        // new screen entirely.
        let id = app_at(&mut comp, 1500, 900, 300, 150);
        comp.maximize_window(id).expect("maximize");
        comp.resize_display(800, 600).expect("resize");

        comp.restore_window(id).expect("restore");

        let frame = comp.window_ref(id).expect("window").frame_rect();
        assert!(
            frame.intersect(&Rect::new(0, 0, 800, 600)).is_some(),
            "un-maximising put the window back at a rectangle that is \
             entirely off the screen: {frame:?}"
        );
    }

    #[test]
    fn leaving_fullscreen_after_a_shrink_leaves_the_window_reachable() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 1500, 900, 300, 150);
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.resize_display(800, 600).expect("resize");

        comp.set_fullscreen(id, false).expect("leave fullscreen");

        let frame = comp.window_ref(id).expect("window").frame_rect();
        assert!(
            frame.intersect(&Rect::new(0, 0, 800, 600)).is_some(),
            "leaving fullscreen put the window back at a rectangle that is \
             entirely off the screen: {frame:?}"
        );
    }

    #[test]
    fn a_restored_window_keeps_its_own_size_and_moves_the_least_it_can() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 1500, 900, 300, 150);
        comp.maximize_window(id).expect("maximize");
        comp.resize_display(800, 600).expect("resize");
        comp.restore_window(id).expect("restore");

        let frame = comp.window_ref(id).expect("window").frame_rect();
        // The rescue never resizes, and it moves by the minimum, so a frame
        // that was off the bottom-right corner lands against that corner at
        // exactly its original size.
        assert_eq!(
            (frame.width, frame.height),
            (302, 181),
            "the restored window was resized rather than moved"
        );
        assert_eq!(
            (frame.x, frame.y),
            (
                800 - i32::try_from(frame.width).expect("frame width"),
                600 - i32::try_from(frame.height).expect("frame height"),
            ),
            "the restored window was moved further than it had to be"
        );
    }

    #[test]
    fn a_restore_rectangle_still_on_the_screen_is_used_exactly() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        let id = app_at(&mut comp, 40, 60, 300, 150);
        let before = comp.window_ref(id).expect("window").frame_rect();
        comp.maximize_window(id).expect("maximize");
        comp.resize_display(800, 600).expect("resize");

        comp.restore_window(id).expect("restore");

        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "a restore rectangle that was still perfectly reachable was moved"
        );
    }

    #[test]
    fn a_restore_rectangle_hanging_off_an_edge_is_still_used_exactly() {
        let mut comp = Compositor::new(1920, 1080, 60).expect("compositor");
        // Straddles both the right and the bottom edge of the *new* screen, so
        // it is partly visible and partly not.
        let id = app_at(&mut comp, 700, 500, 300, 150);
        let before = comp.window_ref(id).expect("window").frame_rect();
        comp.maximize_window(id).expect("maximize");
        comp.resize_display(800, 600).expect("resize");

        comp.restore_window(id).expect("restore");

        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "a window that was still partly on screen -- and still had a title \
             bar to grab -- was tidied onto it anyway"
        );
    }

    #[test]
    fn a_window_restored_on_the_second_monitor_stays_on_it() {
        let (mut comp, id, _) = two_monitors(1);
        let before = comp.window_ref(id).expect("window").frame_rect();
        comp.maximize_window(id).expect("maximize");

        comp.restore_window(id).expect("restore");

        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "restoring a window maximised on the second monitor moved it"
        );
    }

    #[test]
    fn a_window_rescued_on_restore_lands_on_its_own_monitor() {
        let (mut comp, id, screens) = two_monitors(1);
        comp.maximize_window(id).expect("maximize");
        // A saved rectangle that is nowhere on the desktop. In practice that is
        // a resolution change between the maximise and the restore; it is
        // written directly here because a two-monitor `resize_display` can only
        // shrink the *primary*, and it is a window maximised on the second
        // screen that tells the two candidate fallbacks apart.
        comp.window_mut(id).expect("window").restore_rect = Some(Rect::new(9000, 9000, 200, 150));

        comp.restore_window(id).expect("restore");

        let frame = comp.window_ref(id).expect("window").frame_rect();
        assert!(
            frame.intersect(&screens[1]).is_some(),
            "a window maximised on the second monitor was rescued onto the \
             first: {frame:?}"
        );
    }

    // ---- the composited surface is the virtual desktop ----

    #[test]
    fn attaching_a_monitor_grows_the_surface_to_cover_it() {
        // `DisplayManager::add_display` enlarges the virtual desktop, and for
        // as long as that was reachable on its own it enlarged it past the
        // framebuffer everything is composited into. A window on the second
        // monitor was then drawn into pixels beyond the end of the surface and
        // clipped away in full: the model said the window was on the second
        // screen and nothing would ever appear there.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        assert_eq!(comp.frame_size(), (800, 600));
        comp.attach_display(Display::new(1, 1024, 768, 60, 1.0, false))
            .expect("attach");
        let desktop = comp.display_manager.virtual_bounds();
        assert_eq!(
            comp.frame_size(),
            (1824, 768),
            "the surface does not cover the {desktop:?} it is supposed to hold"
        );
    }

    #[test]
    fn resizing_one_monitor_does_not_shrink_the_surface_off_the_other() {
        // The surface follows the *desktop*, and the desktop is the union.
        // Sizing it from the display that changed instead cuts the second
        // monitor's pixels off at the first monitor's new width -- which on a
        // shrink is most of them.
        let (mut comp, _, _) = two_monitors(1);
        comp.resize_display(400, 300).expect("resize");
        assert_eq!(
            comp.frame_size(),
            (1824, 768),
            "shrinking the first monitor took the second monitor's pixels with it"
        );
    }

    #[test]
    fn a_surface_that_cannot_be_allocated_leaves_the_arrangement_alone() {
        // The surface and the display arrangement have to agree, so the
        // allocation is attempted *first*: a compositor that had already
        // adopted a desktop it turned out it could not paint would describe a
        // monitor whose pixels do not exist, which is the very failure the
        // surface-follows-the-desktop rule exists to prevent.
        let mut comp = Compositor::new(800, 600, 60).expect("compositor");
        let before = comp.display_manager.displays().len();
        // Beyond `MAX_DIMENSION`, so the backend refuses it.
        let refused = comp.attach_display(Display::new(1, u32::MAX, 768, 60, 1.0, false));
        assert!(
            refused.is_err(),
            "the backend accepted an impossible surface"
        );
        assert_eq!(
            comp.display_manager.displays().len(),
            before,
            "the display was added even though its pixels could not be"
        );
        assert_eq!(comp.frame_size(), (800, 600), "the surface changed anyway");
    }

    // ---- fullscreen covers one monitor, not the whole scanout surface ----

    #[test]
    fn fullscreen_fills_the_windows_own_monitor_and_not_every_monitor() {
        // The exact bug `maximizing_fills_the_windows_own_monitor_and_not_every_monitor`
        // pins, in the one command that still had it: `set_fullscreen` sized
        // the window from `backend.size()` -- the whole scanout surface -- and
        // put it at (0, 0). So fullscreening a video on the second monitor
        // moved it to the first and stretched it across both, while
        // *maximising* the same window stayed put. The two are the same gesture
        // to a user.
        let (mut comp, id, screens) = two_monitors(1);
        comp.set_fullscreen(id, true).expect("fullscreen");
        assert_eq!(
            comp.window_ref(id).expect("window").client_rect(),
            screens[1],
            "fullscreen did not fill the window's own monitor"
        );
    }

    #[test]
    fn a_fullscreen_window_is_told_its_own_monitors_size() {
        // The client is the only party that can redraw at the new size, and it
        // is told by `WindowResized`. Reporting the framebuffer's dimensions
        // would have a game on the 1024x768 second monitor allocate a
        // 1824x768 surface -- the failure mode is a correctly-placed window
        // full of a wrongly-scaled picture, which is harder to attribute than
        // a window in the wrong place.
        let (mut comp, id, screens) = two_monitors(1);
        comp.pending_notifications.clear();
        comp.set_fullscreen(id, true).expect("fullscreen");
        assert!(
            comp.pending_notifications.iter().any(|n| matches!(
                n,
                EventNotification::WindowResized { window_id, width, height }
                    if *window_id == id
                        && *width == screens[1].width
                        && *height == screens[1].height
            )),
            "the client was told a size that is not its monitor's: {:?}",
            comp.pending_notifications
        );
    }

    #[test]
    fn leaving_fullscreen_on_the_second_monitor_stays_on_it() {
        // The way in was wrong, so the way out could not be checked before
        // this: the window was never on the second monitor to come back to it.
        let (mut comp, id, screens) = two_monitors(1);
        let before = comp.window_ref(id).expect("window").frame_rect();
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.set_fullscreen(id, false).expect("leave");
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "a window fullscreened and un-fullscreened on the second monitor \
             did not come back to where it was"
        );
        assert!(
            before.intersect(&screens[1]).is_some(),
            "the fixture put the window somewhere other than the second \
             monitor, so this proves nothing"
        );
    }

    #[test]
    fn fullscreen_covers_the_taskbar_that_maximize_stops_at() {
        // Fullscreen and maximize resolve the *same* monitor and must then ask
        // it two different questions. `maximize_window` wants the work area, so
        // it stops above a reserved strip; `set_fullscreen` wants the bounds, so
        // it covers it. Routing fullscreen through `work_area_for` -- the
        // obvious tidy-up, since every other tiling path uses it, and the two
        // helpers differ by one word at the call site -- would leave a 40-pixel
        // band of taskbar across the bottom of every full-screen video, which is
        // the one thing fullscreen exists to prevent. On a screen with nothing
        // reserved the two helpers agree, so only a reservation can tell them
        // apart, and every other fullscreen test here has an empty work area.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        let screen = comp.display_manager.displays()[0].bounds();
        let panel = add_panel(&mut comp, screen, 40);
        comp.reserve_edge(panel, PanelEdge::Bottom, 40)
            .expect("reserve");
        let app = app_at(&mut comp, 50, 50, 200, 150);

        comp.maximize_window(app).expect("maximize");
        assert_eq!(
            comp.window_ref(app).expect("window").frame_rect().bottom(),
            screen.bottom() - 40,
            "maximize walked under the taskbar, so the contrast below proves \
             nothing about fullscreen"
        );

        comp.set_fullscreen(app, true).expect("fullscreen");
        assert_eq!(
            comp.window_ref(app).expect("window").client_rect(),
            screen,
            "fullscreen left the reserved strip uncovered"
        );
    }

    #[test]
    fn resizing_one_monitor_leaves_a_fullscreen_window_on_the_other_alone() {
        // `refit_fullscreen_windows` used to take the resized framebuffer's
        // width and height and apply them to *every* fullscreen window. With
        // one screen that is right by construction; with two it moved a game
        // fullscreen on the second monitor onto the first and sent its client
        // a `WindowResized` for a size it does not have.
        let (mut comp, id, screens) = two_monitors(1);
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.pending_notifications.clear();

        comp.resize_display(400, 300).expect("resize");

        assert_eq!(
            comp.window_ref(id).expect("window").client_rect(),
            screens[1],
            "a mode change on the first monitor moved a window fullscreen on \
             the second"
        );
        assert!(
            !comp.pending_notifications.iter().any(
                |n| matches!(n, EventNotification::WindowResized { window_id, .. }
                    if *window_id == id)
            ),
            "the client was told its surface had changed size when it had not"
        );
    }

    #[test]
    fn a_fullscreen_window_on_the_resized_monitor_still_follows_it() {
        // The other half of the pair above: making the re-fit per-monitor must
        // not stop it re-fitting the windows that genuinely are on the monitor
        // that changed.
        let (mut comp, id, _) = two_monitors(0);
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.pending_notifications.clear();

        comp.resize_display(400, 300).expect("resize");

        assert_eq!(
            comp.window_ref(id).expect("window").client_rect(),
            Rect::new(0, 0, 400, 300),
            "a fullscreen window kept its old monitor's size across a mode change"
        );
        assert!(
            comp.pending_notifications.iter().any(|n| matches!(
                n,
                EventNotification::WindowResized {
                    window_id,
                    width: 400,
                    height: 300,
                } if *window_id == id
            )),
            "the client was never told its fullscreen surface had changed size"
        );
    }

    #[test]
    fn the_direct_scanout_bypass_declines_a_second_monitor() {
        // The bypass hands one client's buffer to the scanout surface entire.
        // On a two-headed desktop that surface spans both monitors, so taking
        // it for a window that covers only one would put the game's pixels on
        // the other screen in place of the desktop. The guard is that the
        // window must cover the whole framebuffer, which a one-monitor
        // fullscreen window no longer does -- this pins that the guard is
        // actually load-bearing rather than incidentally true.
        let (mut comp, id, screens) = two_monitors(0);
        comp.set_fullscreen(id, true).expect("fullscreen");
        let (w, h) = (screens[0].width, screens[0].height);
        let bytes = solid_buffer_bytes(w, h, 0xFF00_FF00);
        comp.attach_buffer(id, 1, w, h, w * 4, BufferFormat::Argb8888, &bytes)
            .expect("attach");
        assert_eq!(
            comp.direct_scanout_window(),
            None,
            "the bypass took a window that covers one monitor of two"
        );
    }

    // ---- a monitor leaving is a monitor arriving, in reverse ----

    #[test]
    fn detaching_a_monitor_shrinks_the_desktop_and_the_surface() {
        let (mut comp, _, screens) = two_monitors(1);
        assert_eq!(
            comp.frame_size(),
            (1824, 768),
            "the fixture is not two-headed"
        );

        comp.detach_display(1).expect("detach");

        assert_eq!(comp.display_manager.displays().len(), 1);
        assert_eq!(
            comp.display_manager.virtual_bounds(),
            screens[0],
            "the desktop still spans the monitor that left"
        );
        assert_eq!(
            comp.frame_size(),
            (800, 600),
            "the surface still holds a rectangle nothing scans out"
        );
    }

    #[test]
    fn a_window_maximised_on_the_departed_monitor_comes_back() {
        // The worst case of the lot, and the reason this is not merely untidy: a
        // maximised window has no title bar edge sticking out anywhere, so if it
        // is left on a screen that no longer exists there is nothing on any
        // surviving monitor to drag it back by. It is gone until the session is.
        let (mut comp, id, screens) = two_monitors(1);
        comp.maximize_window(id).expect("maximize");
        assert!(
            comp.window_ref(id).expect("window").frame_rect().x >= screens[1].x,
            "the fixture did not maximise the window onto the second monitor"
        );

        comp.detach_display(1).expect("detach");

        let framed = comp.window_ref(id).expect("window").frame_rect();
        assert_eq!(
            framed,
            work_rect(comp.work_area_for(screens[0])),
            "a maximised window was left on the monitor that was unplugged"
        );
    }

    #[test]
    fn a_hand_placed_window_on_the_departed_monitor_is_rescued() {
        // Not re-laid-out -- the user put it there -- but it does have to end up
        // somewhere reachable, which is the same rule a shrinking mode change
        // already follows.
        let (mut comp, id, screens) = two_monitors(1);
        comp.detach_display(1).expect("detach");
        let framed = comp.window_ref(id).expect("window").frame_rect();
        assert!(
            framed.intersect(&screens[0]).is_some(),
            "a window on the unplugged monitor was left off the desktop: {framed:?}"
        );
    }

    #[test]
    fn a_fullscreen_window_on_the_departed_monitor_is_refitted_and_the_client_told() {
        let (mut comp, id, screens) = two_monitors(1);
        comp.set_fullscreen(id, true).expect("fullscreen");
        comp.pending_notifications.clear();

        comp.detach_display(1).expect("detach");

        assert_eq!(
            comp.window_ref(id).expect("window").client_rect(),
            screens[0],
            "a fullscreen window kept the size of the monitor that left"
        );
        assert!(
            comp.pending_notifications.iter().any(|n| matches!(
                n,
                EventNotification::WindowResized { window_id, width: 800, height: 600 }
                    if *window_id == id
            )),
            "the client was never told its surface had changed size: {:?}",
            comp.pending_notifications
        );
    }

    #[test]
    fn the_pointer_does_not_stay_on_a_monitor_that_left() {
        // The cursor position is not derived from anything -- it is whatever the
        // last motion event said -- so nothing brings it back on its own, and an
        // invisible pointer hit-tests against nothing until the user moves the
        // mouse and the input source volunteers a fresh position.
        let (mut comp, _, screens) = two_monitors(0);
        comp.handle_mouse_move(1500, 400);
        comp.detach_display(1).expect("detach");
        let (x, y) = comp.cursor_position();
        assert!(
            screens[0].contains(x, y),
            "the pointer was left at ({x}, {y}), which is on the monitor that was \
             unplugged"
        );
    }

    #[test]
    fn detaching_the_primary_promotes_a_survivor() {
        // Every "which monitor is this?" question falls back to the primary, so
        // an arrangement with monitors but no primary answers `None` to all of
        // them -- a desktop that has screens and cannot say which one anything
        // is on.
        let (mut comp, _, screens) = two_monitors(1);
        comp.detach_display(0).expect("detach");
        let primary = comp.display_manager.primary().expect("no primary left");
        assert_eq!(primary.id, 1, "the wrong display was promoted");
        assert_eq!(
            primary.bounds(),
            screens[1],
            "promoting the survivor also moved it"
        );
    }

    #[test]
    fn the_survivors_of_a_detach_keep_the_offsets_they_had() {
        // The scanout does not re-flow its surviving heads when one dies
        // (design-decisions.md §515), so this must not either: the two layouts
        // are the same arrangement seen from two sides, and a re-flow on one
        // side alone puts every window on the wrong screen. The visible cost is
        // the hole -- the desktop starts at x = 800 and the surface keeps its
        // full 1824 width with the left 800 columns scanned out nowhere -- and
        // it is the cheaper of the two wrong answers.
        let (mut comp, id, screens) = two_monitors(1);
        let before = comp.window_ref(id).expect("window").frame_rect();

        comp.detach_display(0).expect("detach");

        assert_eq!(
            comp.display_manager.displays()[0].bounds(),
            screens[1],
            "the surviving monitor slid left to fill the gap"
        );
        assert_eq!(
            comp.display_manager.virtual_bounds(),
            screens[1],
            "the desktop is not the one monitor that is left"
        );
        assert_eq!(
            comp.frame_size(),
            (1824, 768),
            "the surface no longer reaches the monitor's right edge"
        );
        assert_eq!(
            comp.window_ref(id).expect("window").frame_rect(),
            before,
            "a window on the surviving monitor was moved by the other one leaving"
        );
    }

    #[test]
    fn a_window_maximised_on_a_monitor_taken_from_the_middle_comes_back() {
        // Three monitors is where "the screen a stranded window is put on" stops
        // being a synonym for "the desktop". Take the middle one away and the
        // desktop is still the bounding box of the other two -- a rectangle
        // spanning the hole, and one that is not any monitor's bounds. Passing
        // *that* to the re-layout makes the re-tile's "windows on the screen
        // that changed" filter match nothing at all, so a maximised window is
        // silently left on the monitor that was unplugged: exactly the failure
        // with no title bar to recover from, reached by a different route. The
        // primary is a real screen and is the answer.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.attach_display(Display::new(1, 640, 480, 60, 1.0, false))
            .expect("attach middle");
        comp.attach_display(Display::new(2, 1024, 768, 60, 1.0, false))
            .expect("attach right");
        let middle = comp.display_manager.displays()[1].bounds();
        let mut spec = WindowSpec::new("In the middle", 200, 150);
        spec.position = Some((middle.x + 100, middle.y + 100));
        let id = comp.create_window_from_spec(&spec, 1);
        comp.maximize_window(id).expect("maximize");

        comp.detach_display(1).expect("detach");

        let framed = comp.window_ref(id).expect("window").frame_rect();
        assert_eq!(
            framed.intersect(&middle),
            None,
            "a maximised window was left in the hole the middle monitor left: \
             {framed:?}"
        );
        assert_eq!(
            framed,
            work_rect(comp.work_area_for(comp.display_manager.displays()[0].bounds())),
            "the window did not come back to a monitor that exists"
        );
    }

    #[test]
    fn detaching_the_last_monitor_is_refused() {
        // Not an arrangement: zero-sized virtual bounds, no primary to fall back
        // to, and every window stranded with nowhere to be rescued to. Keeping
        // the screen the compositor cannot paint on is strictly better than
        // adopting a desktop it cannot describe.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        assert!(
            comp.detach_display(0).is_err(),
            "the only monitor was detached"
        );
        assert_eq!(
            comp.display_manager.displays().len(),
            1,
            "the display went away even though the call failed"
        );
        assert_eq!(comp.frame_size(), (800, 600), "the surface went away too");
    }

    #[test]
    fn detaching_a_display_that_is_not_there_is_an_error_and_not_a_panic() {
        let (mut comp, _, _) = two_monitors(0);
        assert!(
            comp.detach_display(99).is_err(),
            "a display that was never attached was detached anyway"
        );
        assert_eq!(
            comp.display_manager.displays().len(),
            2,
            "a failed detach took a monitor with it"
        );
    }

    // ---- the first screen has to be told what it is plugged into ----

    #[test]
    fn the_first_screen_can_be_told_which_connector_it_is() {
        // `DisplayManager::new` invents the id 0 for the screen the compositor
        // is built at, because nothing has told it what that screen is yet. On
        // a real card the key is the connector id, and the hotplug
        // reconciliation matches the two sets on it -- so a desktop whose first
        // screen is still called 0 has one connector the card reports that the
        // compositor does not recognise (and attaches a duplicate display for)
        // and one display no connector claims (and detaches). Both, once a
        // second, for ever.
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        comp.rename_display(0, 31).expect("rename");
        assert_eq!(comp.display_manager.displays()[0].id, 31);
        assert!(
            comp.display_manager.displays()[0].primary,
            "renaming the screen demoted it"
        );
        assert_eq!(
            comp.display_manager.remove_display(31).map(|d| d.id),
            Some(31),
            "and it answers to the new name"
        );
    }

    #[test]
    fn a_screen_cannot_be_renamed_onto_a_name_another_screen_has() {
        // Two displays sharing an id is not a cosmetic problem: `display_for`,
        // `remove_display` and every reconciliation resolve by id and would
        // silently pick whichever came first, so the wrong monitor goes dark.
        let (mut comp, _, _) = two_monitors(0);
        assert!(
            comp.rename_display(0, 1).is_err(),
            "two displays were given the same id"
        );
        let ids: Vec<u32> = comp
            .display_manager
            .displays()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(ids, vec![0, 1], "a refused rename changed something anyway");
    }

    #[test]
    fn renaming_a_screen_that_is_not_there_is_an_error_and_not_a_panic() {
        let mut comp = Compositor::new(800, 600, 2_000_000).expect("compositor");
        assert!(comp.rename_display(99, 31).is_err());
        assert_eq!(
            comp.display_manager.displays()[0].id,
            0,
            "a failed rename renamed something else"
        );
    }

    // ---- a virtual desktop the compositor can actually show ----

    /// A compositor whose vsync gate is open.
    ///
    /// `should_compose` refuses a frame within one refresh interval of the
    /// last, so at 60 Hz a test that composites twice in a row is told "nothing
    /// was drawn" by the *clock* rather than by the scene -- which would make
    /// "the switch repainted nothing" pass for entirely the wrong reason. A
    /// refresh rate this high rounds the interval to zero.
    fn ungated_compositor(width: u32, height: u32) -> Compositor {
        Compositor::new(width, height, 2_000_000).expect("compositor")
    }

    /// A window filled edge to edge with one opaque colour, so a test can ask
    /// whether it is on screen by looking at the pixel under it.
    ///
    /// A shared buffer rather than a render tree because a buffer covers the
    /// whole client area with known bytes and takes part in the occlusion cull
    /// as an opaque occluder -- which is the half of "is it showing" that a
    /// hit test cannot see.
    ///
    /// **`Xrgb8888`, and that is load-bearing.** `Buffer::is_opaque` is a
    /// question about the *format*, not about the bytes: an `Argb8888` buffer
    /// full of 0xFF alpha is still not an occluder, because the compositor will
    /// not scan a client's pixels to find out. Written the obvious way with
    /// `Argb8888`, `window_opaque_cover` returned `None` for every window here
    /// and the occlusion test passed without ever reaching the cull it names.
    fn painted_window(
        comp: &mut Compositor,
        layer: Layer,
        rect: Rect,
        colour: u32,
    ) -> (WindowId, i32, i32) {
        let mut spec = WindowSpec::new("Painted", rect.width, rect.height);
        spec.position = Some((rect.x, rect.y));
        spec.decorations = false;
        spec.layer = layer;
        let id = comp.create_window_from_spec(&spec, 1);
        let bytes = solid_buffer_bytes(rect.width, rect.height, colour);
        comp.attach_buffer(
            id,
            u64::from(rect.width),
            rect.width,
            rect.height,
            rect.width.saturating_mul(4),
            BufferFormat::Xrgb8888,
            &bytes,
        )
        .expect("attach buffer");
        let win = comp.window_ref(id).expect("window");
        let client = win.client_rect();
        (id, client.x + 1, client.y + 1)
    }

    fn pixel_at(comp: &Compositor, x: i32, y: i32) -> u32 {
        let (w, _) = comp.backend.size();
        let index = usize::try_from(y).expect("y") * usize::try_from(w).expect("w")
            + usize::try_from(x).expect("x");
        comp.backend.presented_pixels()[index]
    }

    /// Build a stack in which a window belonging to *another* desktop sits
    /// **above** a window on the desktop being shown, and return the lower
    /// window's id together with a point inside it.
    ///
    /// **The obvious way to write this does not produce that stack.** Creating
    /// two overlapping windows and moving the upper one away with
    /// `set_window_workspace` leaves the hidden window at the *bottom*: handing
    /// the keyboard on raises the window it lands on (`focus_window` calls
    /// `raise_within_layer`), so the window left showing climbs over the one
    /// just hidden. A hidden window underneath everything can neither occlude
    /// nor overpaint, and both tests below passed against a deliberately broken
    /// cull for exactly that reason — they never reached the code they name.
    ///
    /// A third window on the showing desktop is what fixes it: the focus
    /// handoff raises *that* one, and the window from the other desktop keeps
    /// its place above the one the test looks at. That is also the ordinary
    /// case rather than a contrivance — open something on desktop 2, switch
    /// back to desktop 1, and the desktop-2 window is still stacked above
    /// everything except the window you landed on.
    fn stack_with_a_hidden_window_on_top(
        comp: &mut Compositor,
        colour: u32,
    ) -> (WindowId, i32, i32) {
        let (lower, x, y) = painted_window(comp, Layer::Normal, Rect::new(20, 20, 60, 60), colour);
        // Elsewhere on screen, so its only role is to be the focus target.
        painted_window(comp, Layer::Normal, Rect::new(200, 20, 60, 60), 0xFF00_FF00);
        comp.switch_workspace(1);
        painted_window(
            comp,
            Layer::Normal,
            Rect::new(20, 20, 60, 60),
            HIDDEN_COLOUR,
        );
        comp.switch_workspace(0);
        (lower, x, y)
    }

    /// The colour of the window that is on the desktop the user is *not*
    /// looking at — so a failure message can say which window won.
    const HIDDEN_COLOUR: u32 = 0xFF99_8877;

    #[test]
    fn a_window_on_another_virtual_desktop_is_not_drawn() {
        // The defect this whole feature exists to fix: switching desktop used
        // to change which windows the *taskbar listed* and nothing else, so the
        // window you just left stayed on screen, kept taking clicks, and was no
        // longer reachable from the taskbar -- worse than not having virtual
        // desktops at all.
        let mut comp = ungated_compositor(400, 300);
        let colour = 0xFF11_2233;
        let (_, x, y) = painted_window(&mut comp, Layer::Normal, Rect::new(20, 20, 40, 40), colour);
        assert!(comp.compose_frame(), "nothing was drawn");
        assert_eq!(
            pixel_at(&comp, x, y),
            colour,
            "the window was never on screen"
        );

        comp.switch_workspace(1);
        assert!(comp.compose_frame(), "switching desktop repainted nothing");
        assert_ne!(
            pixel_at(&comp, x, y),
            colour,
            "the window of the desktop we left is still on screen"
        );

        comp.switch_workspace(0);
        assert!(comp.compose_frame(), "switching back repainted nothing");
        assert_eq!(
            pixel_at(&comp, x, y),
            colour,
            "the window did not come back with its desktop"
        );
    }

    #[test]
    fn a_window_on_another_virtual_desktop_does_not_take_clicks() {
        // Drawing and hit testing are separate passes over the same stack, and
        // a window that is invisible but still clickable is the worse of the
        // two failures: the user aims at what they can see and something they
        // cannot see answers.
        let mut comp = ungated_compositor(400, 300);
        let (id, x, y) = painted_window(
            &mut comp,
            Layer::Normal,
            Rect::new(20, 20, 40, 40),
            0xFF11_2233,
        );
        assert_eq!(comp.window_at(x, y), Some(id), "the window was never hit");
        assert_eq!(
            comp.window_at_with_decorations(x, y),
            Some(id),
            "the window's frame was never hit"
        );

        comp.switch_workspace(1);
        assert_eq!(
            comp.window_at(x, y),
            None,
            "a window on another desktop swallowed a click"
        );
        assert_eq!(
            comp.window_at_with_decorations(x, y),
            None,
            "a window on another desktop swallowed a click on its frame"
        );
    }

    #[test]
    fn a_window_on_another_virtual_desktop_does_not_occlude_the_one_in_front_of_you() {
        // The occlusion cull decides what *not* to draw by asking which
        // rectangles are opaquely covered. A hidden window left in that answer
        // is the most confusing possible symptom: the window you are looking at
        // is genuinely on this desktop and genuinely mapped, and a rectangular
        // hole appears in it where something on another desktop used to be.
        let mut comp = ungated_compositor(400, 300);
        let under = 0xFF11_2233;
        let (_, x, y) = stack_with_a_hidden_window_on_top(&mut comp, under);

        assert!(comp.compose_frame(), "nothing was drawn");
        assert_eq!(
            pixel_at(&comp, x, y),
            under,
            "the window in front of the user was culled away by one on another desktop"
        );
    }

    #[test]
    fn the_taskbar_is_on_every_virtual_desktop() {
        // A taskbar that vanished on a switch would take the only means of
        // switching back with it. Layer is the property that says so: the
        // wallpaper and the shell's chrome belong to the screen, not to a
        // desktop.
        let mut comp = ungated_compositor(400, 300);
        let colour = 0xFF44_5566;
        let (bar, x, y) = painted_window(
            &mut comp,
            Layer::Overlay,
            Rect::new(0, 270, 400, 30),
            colour,
        );
        comp.switch_workspace(3);
        assert!(comp.compose_frame(), "nothing was drawn");
        assert_eq!(
            pixel_at(&comp, x, y),
            colour,
            "the taskbar went away with the desktop it was created on"
        );
        assert_eq!(
            comp.window_at(x, y),
            Some(bar),
            "the taskbar is drawn but no longer takes clicks"
        );
    }

    #[test]
    fn an_assignment_a_panel_ignores_is_stored_rather_than_refused() {
        // A shell moving "all its windows" must not have to know which of them
        // are furniture. The assignment is taken and remembered; `is_showing`
        // is what ignores it.
        let mut comp = ungated_compositor(400, 300);
        let (bar, x, y) = painted_window(
            &mut comp,
            Layer::Overlay,
            Rect::new(0, 270, 400, 30),
            0xFF44_5566,
        );
        comp.set_window_workspace(bar, 2).expect("move");
        assert_eq!(
            comp.window_ref(bar).expect("window").workspace,
            2,
            "the assignment was dropped instead of stored"
        );
        assert_eq!(
            comp.window_at(x, y),
            Some(bar),
            "a panel told it was on desktop 2 left desktop 0"
        );
    }

    #[test]
    fn switching_desktop_takes_the_keyboard_off_the_window_it_hides() {
        // A window nobody can see must not hold the keyboard: every keystroke
        // would go somewhere the user cannot find, and nothing on screen would
        // say where.
        let mut comp = ungated_compositor(400, 300);
        let first = comp.create_window("First".to_string(), 100, 80, 1);
        comp.switch_workspace(1);
        let second = comp.create_window("Second".to_string(), 100, 80, 1);
        assert_eq!(
            comp.focused_window,
            Some(second),
            "the new window is focused"
        );

        comp.switch_workspace(0);
        assert_eq!(
            comp.focused_window,
            Some(first),
            "the keyboard did not follow the screen"
        );
        assert!(
            !comp.window_ref(second).expect("window").focused,
            "a window on another desktop still believes it has the keyboard"
        );
    }

    #[test]
    fn switching_to_an_empty_desktop_focuses_nothing() {
        // Not "keeps the last window focused", which is the tempting shortcut:
        // an empty desktop genuinely has nothing to type into, and saying so is
        // what stops keystrokes reaching a window on a desktop nobody is
        // looking at.
        let mut comp = ungated_compositor(400, 300);
        let id = comp.create_window("Only".to_string(), 100, 80, 1);
        assert_eq!(comp.focused_window, Some(id));
        comp.switch_workspace(1);
        assert_eq!(
            comp.focused_window, None,
            "an empty desktop kept the keyboard on a window it does not have"
        );
    }

    #[test]
    fn moving_the_focused_window_away_hands_the_keyboard_on() {
        // The other way a focused window can leave the screen. It goes through
        // the same rule as a switch rather than a second one, because two rules
        // for "what has the keyboard now" is how the two answers drift.
        let mut comp = ungated_compositor(400, 300);
        let staying = comp.create_window("Staying".to_string(), 100, 80, 1);
        let leaving = comp.create_window("Leaving".to_string(), 100, 80, 1);
        assert_eq!(comp.focused_window, Some(leaving));
        comp.set_window_workspace(leaving, 1).expect("move");
        assert_eq!(
            comp.focused_window,
            Some(staying),
            "the keyboard stayed on a window that left the desktop"
        );
    }

    #[test]
    fn a_window_opens_on_the_desktop_the_user_is_looking_at() {
        // Not on desktop 0, which is what a `Default`-derived field would give
        // it: a program started from the desktop you are on must appear on the
        // desktop you are on. A client cannot name a workspace in its spec at
        // all -- see `create_window_from_spec`.
        let mut comp = ungated_compositor(400, 300);
        comp.switch_workspace(2);
        let id = comp.create_window("New".to_string(), 100, 80, 1);
        assert_eq!(
            comp.window_ref(id).expect("window").workspace,
            2,
            "a window opened on a desktop the user was not looking at"
        );
        assert_eq!(
            comp.focused_window,
            Some(id),
            "a window opened on this desktop did not get the keyboard"
        );
    }

    #[test]
    fn activating_a_window_on_another_desktop_goes_to_it_rather_than_dragging_it_here() {
        // Activating is a request to *see* a particular window. Switching moves
        // one thing and is undone by switching back; dragging the window here
        // rearranges the desktops themselves and leaves the user to notice.
        let mut comp = ungated_compositor(400, 300);
        let away = comp.create_window("Away".to_string(), 100, 80, 1);
        comp.switch_workspace(1);
        comp.create_window("Here".to_string(), 100, 80, 1);

        comp.activate_window(away).expect("activate");
        assert_eq!(
            comp.current_workspace(),
            0,
            "activating a window elsewhere did not follow it"
        );
        assert_eq!(
            comp.window_ref(away).expect("window").workspace,
            0,
            "the window was dragged to the current desktop instead"
        );
        assert_eq!(
            comp.focused_window,
            Some(away),
            "followed the window but did not focus it"
        );
    }

    #[test]
    fn a_window_the_user_cannot_see_is_not_handed_the_whole_screen() {
        // The direct-scanout bypass hands one window's buffer straight to the
        // display. A fullscreen window on another desktop passing that test
        // would put the desktop you left back on screen in its entirety, with
        // nothing else composited over it at all.
        let mut comp = ungated_compositor(64, 48);
        let mut spec = WindowSpec::new("Game", 64, 48);
        spec.position = Some((0, 0));
        spec.decorations = false;
        let id = comp.create_window_from_spec(&spec, 1);
        comp.set_fullscreen(id, true).expect("fullscreen");
        let bytes = solid_buffer_bytes(64, 48, 0xFF00_FF00);
        comp.attach_buffer(id, 7, 64, 48, 256, BufferFormat::Argb8888, &bytes)
            .expect("attach");
        assert_eq!(
            comp.direct_scanout_window(),
            Some(id),
            "the bypass never took this window"
        );

        comp.switch_workspace(1);
        assert_eq!(
            comp.direct_scanout_window(),
            None,
            "a window on another desktop was scanned out over the one in front of the user"
        );
    }

    #[test]
    fn a_window_on_another_desktop_is_still_in_the_window_list() {
        // The list is what a taskbar is built from, and a taskbar has to be
        // able to show the other desktops' windows -- that is how the user
        // finds them again. Hiding is a compositing decision, not a reason to
        // deny the window exists.
        let mut comp = ungated_compositor(400, 300);
        let id = comp.create_window("Away".to_string(), 100, 80, 1);
        comp.switch_workspace(1);
        assert!(
            comp.window_list().windows.iter().any(|w| w.id == id.raw()),
            "a window on another desktop disappeared from the window list"
        );
    }

    #[test]
    fn the_window_list_reports_where_each_window_actually_is() {
        // An overview draws thumbnails in proportion to the real windows, so
        // the rectangle in the list has to be the window's own -- not a
        // placeholder, and not the previous frame's. A list that reported
        // zero-by-zero would draw an empty desktop over a full one, which reads
        // as "nothing is running" rather than as a bug.
        let mut comp = ungated_compositor(1920, 1080);
        let id = comp.create_window("Placed".to_string(), 640, 480, 1);
        comp.move_window(id, -100, 250).expect("move");

        let list = comp.window_list();
        let w = list
            .windows
            .iter()
            .find(|w| w.id == id.raw())
            .expect("the window is listed");
        let real = comp.window_ref(id).expect("window");
        assert_eq!(
            (w.x, w.y, w.width, w.height),
            (real.x, real.y, real.width, real.height),
            "the list's rectangle is not the window's"
        );
        // Stated separately from the equality above, because that one would
        // still hold if both sides were zero. The move must be visible.
        assert_eq!((w.x, w.y), (-100, 250));
    }

    #[test]
    fn moving_a_window_moves_it_in_the_next_window_list() {
        // The list is rebuilt per call rather than cached, and this is the test
        // that says so: a cache that filled once at creation would pass every
        // other geometry assertion here and go stale the instant a user dragged
        // a window.
        let mut comp = ungated_compositor(1920, 1080);
        let id = comp.create_window("Dragged".to_string(), 300, 200, 1);
        comp.move_window(id, 10, 20).expect("move");
        let before = comp.window_list();
        comp.move_window(id, 700, 400).expect("move again");
        let after = comp.window_list();

        let pos = |list: &WindowList| {
            let w = list
                .windows
                .iter()
                .find(|w| w.id == id.raw())
                .expect("listed");
            (w.x, w.y)
        };
        assert_eq!(pos(&before), (10, 20));
        assert_eq!(pos(&after), (700, 400), "the list did not follow the move");
    }

    #[test]
    fn a_window_comes_back_from_another_desktop_exactly_as_it_was_left() {
        // Hiding must be *only* hiding. Minimizing or unmapping the departing
        // windows would be visible on the way back -- a maximized window that
        // came back restored, a client told to redraw for a switch it has no
        // business hearing about.
        let mut comp = ungated_compositor(400, 300);
        let id = comp.create_window("Kept".to_string(), 100, 80, 1);
        comp.maximize_window(id).expect("maximize");
        let before = comp.window_ref(id).expect("window").clone();

        comp.switch_workspace(1);
        let hidden = comp.window_ref(id).expect("window");
        assert!(!hidden.minimized, "hiding a desktop minimized its windows");
        assert!(hidden.visible, "hiding a desktop unmapped its windows");

        comp.switch_workspace(0);
        let after = comp.window_ref(id).expect("window");
        assert_eq!(
            (after.x, after.y, after.width, after.height, after.maximized),
            (
                before.x,
                before.y,
                before.width,
                before.height,
                before.maximized
            ),
            "the window came back changed"
        );
    }

    #[test]
    fn switching_to_the_desktop_already_showing_is_not_a_repaint() {
        // A shell re-asserting its state on every frame is a normal thing for a
        // shell to do. If that forced a full recomposite the desktop would
        // repaint entirely, every frame, for ever.
        let mut comp = ungated_compositor(400, 300);
        comp.create_window("Any".to_string(), 100, 80, 1);
        assert!(comp.compose_frame(), "nothing was drawn");
        comp.switch_workspace(comp.current_workspace());
        assert!(
            !comp.full_recomposite,
            "re-asserting the current desktop asked for a full repaint"
        );
        assert!(
            !comp.compose_frame(),
            "re-asserting the current desktop redrew the frame"
        );
    }

    #[test]
    fn the_damage_path_does_not_repaint_a_window_from_another_desktop() {
        // A switch asks for a full recomposite, so every other test in this
        // section goes through `render_all_windows` and never reaches the
        // *damage* pass -- which is a second, independent walk of the same z
        // stack, with its own copy of the "is it showing" question. A window
        // moving on the desktop you are looking at is what drives it, and a
        // window from another desktop overlapping the damaged rectangle is what
        // it would wrongly repaint.
        let mut comp = ungated_compositor(400, 300);
        let under = 0xFF11_2233;
        let (here, x, y) = stack_with_a_hidden_window_on_top(&mut comp, under);
        assert!(comp.compose_frame(), "nothing was drawn");
        assert!(
            !comp.full_recomposite,
            "the first frame left a full recomposite pending, so the damage pass would be skipped"
        );

        comp.damage_window(here);
        assert!(comp.compose_frame(), "a damaged window redrew nothing");
        assert_eq!(
            pixel_at(&comp, x, y),
            under,
            "the damage pass repainted a window belonging to another desktop"
        );
    }

    // ---- a rectangle wider than i32 is wide, not negative ----

    #[test]
    fn unioning_an_over_wide_rect_does_not_shrink_the_bounding_box() {
        // `union` used to compute its far edge as `x + width as i32`. A width
        // above `i32::MAX` casts to a negative number, so the bounding box came
        // out *smaller* than the rectangle it was supposed to contain -- and
        // the caller that noticed was `attach_display`, which sized the scanout
        // surface from the union and so accepted a desktop no framebuffer could
        // hold. `right()` already saturated correctly; `union` now uses it.
        let sane = Rect::new(0, 0, 800, 600);
        let huge = Rect::new(0, 0, u32::MAX, 600);
        let bounds = sane.union(&huge);
        assert!(
            bounds.width >= sane.width,
            "the union of a rectangle with a wider one came out narrower than \
             the first: {bounds:?}"
        );
        assert_eq!(
            bounds.width, 2_147_483_647,
            "an over-wide rectangle should pin the bounding box at the widest \
             representable one, not wrap"
        );
    }

    #[test]
    fn intersecting_an_over_wide_rect_does_not_come_back_empty() {
        // The same cast in `intersect` put the far edge left of the near one,
        // so a rectangle that contains everything intersected to nothing --
        // which would silently clip away every window on such a display.
        let huge = Rect::new(0, 0, u32::MAX, u32::MAX);
        let window = Rect::new(100, 100, 200, 150);
        assert_eq!(
            huge.intersect(&window),
            Some(window),
            "a rectangle inside an over-wide one was reported as not \
             overlapping it"
        );
    }
}
