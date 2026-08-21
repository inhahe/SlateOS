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
use guiremote::control::WindowSpec;
use guiremote::scene::{SceneFrame, SceneSession, WindowSnapshot};

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

/// Size, in pixels, for text the compositor draws itself — window titles and
/// the like. Text inside a window carries its own size in the render command.
const DEFAULT_FONT_SIZE: f32 = 16.0;

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
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self
            .x
            .saturating_add(self.width as i32)
            .min(other.x.saturating_add(other.width as i32));
        let y2 = self
            .y
            .saturating_add(self.height as i32)
            .min(other.y.saturating_add(other.height as i32));

        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, span(x1, x2), span(y1, y2)))
        } else {
            None
        }
    }

    /// Compute the bounding box that contains both rectangles.
    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self
            .x
            .saturating_add(self.width as i32)
            .max(other.x.saturating_add(other.width as i32));
        let y2 = self
            .y
            .saturating_add(self.height as i32)
            .max(other.y.saturating_add(other.height as i32));

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
// Window
// ---------------------------------------------------------------------------

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
    /// Position and size before maximizing (for restore).
    pub restore_rect: Option<Rect>,
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
            dirty: true,
            decorations: spec.decorations,
            resizable: spec.resizable,
            transparent: spec.transparent,
            min_size: spec.min_size,
            max_size: spec.max_size,
            cursor: CursorShape::Arrow,
        }
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
    pub const fn frame_insets(&self) -> (u32, u32, u32) {
        if self.is_framed() {
            (TITLE_BAR_HEIGHT, BORDER_WIDTH, BORDER_WIDTH)
        } else {
            (0, 0, 0)
        }
    }

    /// How far the drop shadow extends beyond the frame. Zero when unframed:
    /// the shadow is part of the decoration, not of the window.
    pub const fn shadow_extent(&self) -> u32 {
        if self.is_framed() { SHADOW_SIZE } else { 0 }
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
        let (top, side, bottom) = self.frame_insets();
        Rect::new(
            self.x.saturating_sub(side as i32),
            self.y.saturating_sub(top as i32),
            self.width.saturating_add(side.saturating_mul(2)),
            self.height.saturating_add(top).saturating_add(bottom),
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
        let step = (TITLE_BUTTON_SIZE.saturating_add(TITLE_BUTTON_SPACING)) as i32;
        let btn_x = title_rect
            .x
            .saturating_add(title_rect.width as i32)
            .saturating_sub(TITLE_BUTTON_SIZE as i32)
            .saturating_sub(TITLE_BUTTON_SPACING as i32)
            .saturating_sub((slot as i32).saturating_mul(step));
        let btn_y = title_rect.y.saturating_add(
            (title_rect.height as i32).saturating_sub(TITLE_BUTTON_SIZE as i32) / 2,
        );
        Some(Rect::new(
            btn_x,
            btn_y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
        ))
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

    /// Get the total virtual desktop bounds (union of all displays).
    pub fn virtual_bounds(&self) -> Rect {
        // Taken from the iterator rather than by `[0]` after an `is_empty()`
        // guard: the guard and the index are two statements that have to agree
        // about the same fact, and only one of them is checked by the compiler.
        let mut rest = self.displays.iter();
        let Some(first) = rest.next() else {
            return Rect::new(0, 0, 0, 0);
        };
        rest.fold(first.bounds(), |bounds, display| {
            bounds.union(&display.bounds())
        })
    }

    /// Get all displays.
    pub fn displays(&self) -> &[Display] {
        &self.displays
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
    /// Begin a remote draw-command stream session (returns a stream id).
    StreamStart,
    /// Capture the current scene for a stream session as an encoded wire frame.
    StreamCapture { stream_id: u64 },
    /// End a remote draw-command stream session.
    StreamStop { stream_id: u64 },
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
        character: Option<char>,
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
            character,
        } => guiremote::InputEvent::key(
            window_id.0,
            ClientKeyEvent {
                key,
                pressed,
                modifiers,
                text: character,
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
                corner_radii: _,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                self.fill_rect(fb, px, py, w, h, color_to_argb(color), opacity);
            }
            RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                color,
                line_width,
                corner_radii: _,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                let lw = (*line_width).max(1.0) as u32;
                self.stroke_rect(fb, px, py, w, h, lw, color_to_argb(color), opacity);
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
                corner_radii: _,
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
                self.fill_rect(fb, px, py, w, h, color_to_argb(color), opacity);
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

/// Convert a guitk Color to ARGB u32 format.
#[inline]
fn color_to_argb(color: &Color) -> u32 {
    (color.a as u32) << 24 | (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

// ---------------------------------------------------------------------------
// Theme colors for window decorations
// ---------------------------------------------------------------------------

/// Colors used for window decoration rendering.
#[allow(dead_code)]
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
    /// Close button hover color.
    close_button_hover: u32,
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

impl Default for DecorationTheme {
    fn default() -> Self {
        Self {
            title_bar_focused: 0xFF_2B_2B_3D,    // Dark blue-gray
            title_bar_unfocused: 0xFF_3C_3C_4A,  // Lighter gray
            title_text_focused: 0xFF_FF_FF_FF,   // White
            title_text_unfocused: 0xFF_A0_A0_A0, // Gray text
            close_button: 0xFF_E8_4D_4D,         // Red
            close_button_hover: 0xFF_FF_60_60,   // Bright red
            maximize_button: 0xFF_4D_C8_4D,      // Green
            minimize_button: 0xFF_E8_C8_4D,      // Yellow
            border_focused: 0xFF_50_50_70,       // Subtle border
            border_unfocused: 0xFF_40_40_50,     // Dimmer border
            shadow_color: 0x40_00_00_00,         // Semi-transparent black
            desktop_background: 0xFF_1A_1A_2E,   // Dark navy
        }
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
    /// Rendering engine instance.
    render_engine: RenderEngine,
    /// Decoration theme.
    theme: DecorationTheme,
    /// Outbound event notifications for clients (stub queue).
    pending_notifications: VecDeque<EventNotification>,
    /// Which modifier keys are held, and whether Caps Lock is latched.
    ///
    /// Kept here rather than derived per event because a modifier is a *state*
    /// spanning two events — a press and a release, arbitrarily far apart —
    /// and the chord a client is told about is the state at the moment the
    /// other key went down. Held centrally for the same reason the keymap is
    /// (`design-decisions.md` §456): one answer for the whole system, so two
    /// applications cannot disagree about whether Ctrl was down.
    modifiers: ModifierState,
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
            render_engine: RenderEngine::new(),
            theme: DecorationTheme::default(),
            pending_notifications: VecDeque::new(),
            modifiers: ModifierState::new(),
            full_recomposite: true,
            occlusion_cull: true,
            scanout: Scanout::Composited,
            stream_sessions: BTreeMap::new(),
            next_stream_id: 1,
        })
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

    /// Maximize a window to fill the display.
    ///
    /// Refused for a window the client declared non-resizable: maximising is a
    /// resize, and a window that said it only works at one size means it.
    pub fn maximize_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        if !self
            .window_ref(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?
            .resizable
        {
            return Err(CompositorError::NotResizable(window_id));
        }

        self.damage_window(window_id);

        let display_bounds = self.display_manager.virtual_bounds();

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            if !window.maximized {
                // Save current geometry for restore.
                window.restore_rect =
                    Some(Rect::new(window.x, window.y, window.width, window.height));
            }

            window.maximized = true;
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

    /// Restore a window from minimized or maximized state.
    pub fn restore_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;

        if window.minimized {
            window.minimized = false;
            window.visible = true;
        }

        if window.maximized {
            window.maximized = false;
            if let Some(restore) = window.restore_rect.take() {
                window.x = restore.x;
                window.y = restore.y;
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
    /// the client area to cover the entire display. Leaving restores the saved
    /// geometry. A fullscreen window with an opaque, display-sized shared
    /// buffer is eligible for direct-scanout bypass (see [`compose_frame`]).
    ///
    /// [`compose_frame`]: Compositor::compose_frame
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist.
    pub fn set_fullscreen(&mut self, window_id: WindowId, enable: bool) -> CompositorResult<()> {
        self.damage_window(window_id);

        let (fb_w, fb_h) = self.backend.size();

        let resized = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            if enable {
                if !window.fullscreen {
                    window.fs_restore_rect =
                        Some(Rect::new(window.x, window.y, window.width, window.height));
                }
                window.fullscreen = true;
                window.x = 0;
                window.y = 0;
                window.width = fb_w;
                window.height = fb_h;
                window.dirty = true;
                Some((fb_w, fb_h))
            } else if window.fullscreen {
                window.fullscreen = false;
                let restored = window.fs_restore_rect.take();
                if let Some(r) = restored {
                    window.x = r.x;
                    window.y = r.y;
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
    fn direct_scanout_window(&self) -> Option<WindowId> {
        // Topmost visible window in z-order (z_stack top == last).
        let &top = self.z_stack.iter().rev().find(|&&id| {
            self.window_ref(id)
                .is_some_and(|w| w.visible && !w.minimized)
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
            if let Some(win) = self.window_mut(old_id) {
                win.focused = false;
                win.dirty = true;
            }
            self.damage_window(old_id);
            self.pending_notifications
                .push_back(EventNotification::FocusLost { window_id: old_id });
        }

        // Focus the new window.
        if let Some(win) = self.window_mut(window_id)
            && !win.minimized
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

    fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Release ends any active drag.
        if !pressed && button == MouseButton::Left {
            self.drag = None;
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
                    // Title bar drag?
                    if win.title_bar_rect().is_some_and(|r| r.contains(x, y)) {
                        self.drag = Some(DragState {
                            window_id,
                            mode: DragMode::MoveWindow,
                            start_mouse: Point::new(x, y),
                            start_window_pos: Point::new(win.x, win.y),
                            start_window_size: (win.width, win.height),
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
        self.pending_notifications
            .push_back(EventNotification::KeyEvent {
                window_id,
                scancode,
                key: key_for_scancode(scancode),
                pressed,
                modifiers: self.modifiers.modifiers(),
                character,
            });
    }

    /// Release every held modifier.
    ///
    /// Call when the compositor loses the keyboard — session switch, device
    /// unplug, VT change. Without it a modifier held at that moment stays held
    /// forever and every later keystroke arrives as a chord: the classic
    /// "stuck Ctrl" that makes a desktop look crashed when nothing has
    /// actually failed.
    pub const fn release_all_modifiers(&mut self) {
        self.modifiers.release_all();
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
                && win.visible
                && !win.minimized
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
                && win.visible
                && !win.minimized
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
            if !win.visible || win.minimized {
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

    // -----------------------------------------------------------------------
    // Compositing pipeline
    // -----------------------------------------------------------------------

    /// Composite a frame. Returns true if a frame was actually composited
    /// (false if skipped due to no damage or frame budget).
    pub fn compose_frame(&mut self) -> bool {
        // Check if we should compose (frame timing).
        if !self.frame_stats.should_compose() {
            return false;
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
            .filter_map(Self::window_opaque_cover)
            .collect()
    }

    /// The screen-space rectangle this one window is guaranteed to overwrite
    /// with fully opaque pixels, if any.
    ///
    /// Shared by the background-clear cull ([`opaque_cover_rects`](Self::opaque_cover_rects))
    /// and the inter-window cull in [`render_all_windows`](Self::render_all_windows),
    /// so the two can never disagree about what counts as opaque — a window
    /// treated as an occluder by one and not the other would leave a hole.
    fn window_opaque_cover(win: &Window) -> Option<Rect> {
        if !win.visible || win.minimized || win.opacity < 1.0 {
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
    /// layer (`SHADOW_SIZE - 1` out, cast 3 px down-right) and the border
    /// stroke (one out), with room to spare in every direction.
    fn window_drawn_extent(win: &Window) -> Rect {
        win.frame_rect()
            .inflate(SHADOW_SIZE.saturating_add(BORDER_WIDTH).saturating_add(3))
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
            .map(|&id| self.window_ref(id).and_then(Self::window_opaque_cover))
            .collect();

        for (idx, &window_id) in z_stack_copy.iter().enumerate() {
            let Some(win) = self.window_ref(window_id) else {
                continue;
            };
            if !win.visible || win.minimized {
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
                if !win.visible || win.minimized {
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
            Some(win) if win.visible && !win.minimized => (
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
        ) = win_data;

        // Undecorated and fullscreen windows get no frame: the first asked to
        // be a bare surface (a menu, a tooltip, a splash screen), the second
        // owns the whole display. Both report it by having no title bar.
        if let Some(bar) = title_bar {
            // 1. Draw window shadow.
            self.render_shadow(bar.frame, opacity);

            // 2. Draw window border.
            let border_color = if focused {
                self.theme.border_focused
            } else {
                self.theme.border_unfocused
            };
            self.render_border(bar.frame, border_color, opacity);

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
    fn render_shadow(&mut self, frame: Rect, opacity: f32) {
        /// How far down and right the shadow is cast from the frame.
        const SHADOW_OFFSET: i32 = 3;
        /// Alpha of the innermost shadow layer, falling off per layer.
        const SHADOW_ALPHA: u32 = 40;
        const SHADOW_FALLOFF: u32 = 5;

        let base = frame.offset(SHADOW_OFFSET, SHADOW_OFFSET);
        for layer in 0..SHADOW_SIZE {
            let alpha = SHADOW_ALPHA
                .saturating_sub(layer.saturating_mul(SHADOW_FALLOFF))
                .min(255);
            let ring = base.inflate(layer);
            // Only the outline of each layer: the interior is covered by the
            // window itself or by the next layer in.
            self.render_engine.stroke_rect(
                &mut self.backend,
                ring.x,
                ring.y,
                ring.width,
                ring.height,
                1,
                alpha << 24,
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
    fn render_border(&mut self, frame: Rect, color: u32, opacity: f32) {
        let border = Rect::new(
            frame.x,
            frame.y.saturating_sub(BORDER_WIDTH as i32),
            frame.width,
            frame.height.saturating_add(BORDER_WIDTH),
        );
        self.render_engine.stroke_rect(
            &mut self.backend,
            border.x,
            border.y,
            border.width,
            border.height,
            BORDER_WIDTH,
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
        self.render_engine.fill_rect(
            &mut self.backend,
            tb_x,
            tb_y,
            tb_width,
            bar.bar.height,
            bg_color,
            opacity,
        );

        // Title text.
        let text_color = if focused {
            self.theme.title_text_focused
        } else {
            self.theme.title_text_unfocused
        };
        /// Gap between the left edge of the title bar and the title text.
        const TITLE_TEXT_INSET: u32 = 8;
        let text_x = tb_x.saturating_add(TITLE_TEXT_INSET as i32);
        // Centred on the font's own line height rather than a hardcoded cell
        // size, so the title stays centred if the title-bar font ever changes.
        let line_height = self
            .render_engine
            .fonts
            .get(DEFAULT_FONT_SIZE, Weight::Regular, Family::Ui)
            .line_height();
        let text_y =
            tb_y.saturating_add((bar.bar.height as i32).saturating_sub(line_height as i32) / 2);
        // Reserve exactly the buttons this window actually has, so a window
        // with no maximize button gets that space for its title instead of
        // eliding text to make room for nothing.
        let buttons = TITLE_BUTTON_SIZE
            .saturating_add(TITLE_BUTTON_SPACING)
            .saturating_mul(bar.button_count());
        let max_text_width =
            tb_width.saturating_sub(buttons.saturating_add(TITLE_TEXT_INSET.saturating_mul(2)));
        self.render_engine.draw_text(
            &mut self.backend,
            text_x,
            text_y,
            title,
            text_color,
            &[],
            opacity,
            Some(max_text_width),
            DEFAULT_FONT_SIZE,
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
        for (rect, color) in [
            (bar.close, self.theme.close_button),
            (bar.maximize, self.theme.maximize_button),
            (bar.minimize, self.theme.minimize_button),
        ] {
            if let Some(r) = rect {
                self.render_engine.fill_rect(
                    &mut self.backend,
                    r.x,
                    r.y,
                    r.width,
                    r.height,
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
    pub fn resize_display(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        self.backend.resize(width, height)?;

        // Update the primary display.
        if let Some(display) = self.display_manager.displays.first_mut() {
            display.width = width;
            display.height = height;
        }

        self.full_recomposite = true;
        self.damage.mark_full(width, height);
        Ok(())
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
                if !win.visible || win.minimized {
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
                .is_some_and(|w| w.visible && !w.minimized)
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
        assert_eq!(k.text, Some('a'));
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
}
