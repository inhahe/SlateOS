//! Screen Magnifier — the accessibility zoom for SlateOS, in a real window.
//!
//! What it does: it shows a magnified view of the screen, either filling the
//! window, or under a lens that follows the pointer, or docked into a strip at
//! the top or the bottom with the rest of the screen at life size. On top of
//! that view it can put a colour filter (inversion, three high-contrast pairs,
//! greyscale, and simulations of the three dichromacies), a crosshair on the
//! point being magnified, a ruler between two points, and a colour readout for
//! any pixel you point at.
//!
//! ## What wiring it to a window found
//!
//! It drew all of that before, and none of it ever appeared, because `main`
//! built a `MagnifierApp` and dropped it. Everything below the drawing was
//! written against a window that did not exist, and ten faults had grown in
//! the gap between what the code said and what nothing was checking.
//!
//! 1. **No window.** `fn main() { let _app = MagnifierApp::new(); }` — the
//!    program constructed its state, threw it away, and exited 0. It now runs
//!    through [`app::launch`].
//!
//! 2. **The layout was two constants.** `width: 800.0, height: 600.0` were
//!    fields set once in `new` and never assigned again by anything. Every
//!    rectangle in the drawing pass was derived from them, so in any window
//!    that was not 800x600 the toolbar, the colour readout and the help sheet
//!    were drawn off the edge or floating in the middle. [`Layout`] is now
//!    built fresh from the live window size on every frame and is never stored.
//!
//! 3. **No pointer input at all.** There was no mouse handler. `update_mouse`
//!    existed and nothing called it, so lens mode — whose entire premise is a
//!    lens that follows the pointer — could not move, and the toolbar's
//!    `[H]elp [M]ode [T]rack [F]ilter` was a picture of buttons. Every control
//!    is now a recorded hit box, put there by the drawing pass itself, so a
//!    button is clickable exactly where it is drawn.
//!
//! 4. **Keys arrived as strings that nothing produced.** `handle_key(key:
//!    &str, ctrl: bool, shift: bool)` matched `"Left"`, `"F1"`, `"="`. A window
//!    delivers a [`KeyEvent`] carrying a [`Key`] and a `pressed` flag, so not
//!    one of those arms could ever have fired. And with no `pressed` check,
//!    every keystroke would have been counted twice — once going down and once
//!    coming back up.
//!
//! 5. **The lens could not be resized, provably.** The four `"Left" if shift`
//!    arms sat *below* the four `"Left" if !ctrl` arms. Shift-Left has `ctrl`
//!    false, so the earlier arm always won and the later four were unreachable.
//!    The compiler would have said so, except for the `#![allow(dead_code)]` on
//!    line 1 — which is why that is gone too.
//!
//! 6. **The colour readout could never be dismissed.** `show_color_picker` was
//!    set true by `pick_color_at_center` and assigned false nowhere in the
//!    program. Once shown it covered the bottom-right corner for the life of
//!    the process. It is now a toggle, and it is a band in the layout rather
//!    than a panel laid over one — the old one overlapped the toolbar exactly.
//!
//! 7. **Pausing paused only the picture.** `enabled` was read in `render` and
//!    nowhere else, so a "paused" magnifier still zoomed, panned, cycled
//!    filters and picked colours — it just did not show you any of it. Pausing
//!    now stops the input too, except for the key that resumes.
//!
//! 8. **The lens showed somewhere other than where it was.** `render_lens`
//!    positioned the lens at `mouse_x/mouse_y` and filled it from
//!    `center_x/center_y`. Those two are the same number only while tracking
//!    follows the mouse; in `Manual` tracking the lens sat under the pointer
//!    and showed a different part of the screen entirely.
//!
//! 9. **The ruler measured in the wrong direction and drew a horizontal bar.**
//!    `screen_distance` divided the length by the zoom and the status line
//!    called the result "at zoom" — but the endpoints are screen coordinates,
//!    so the on-screen length is the *product*, not the quotient. The drawing
//!    ignored `end_y` altogether and drew a two-pixel-tall rectangle from
//!    `min_x` to `max_x` in screen coordinates, in a window that is not the
//!    screen.
//!
//! 10. **The magnified view could hang off the screen.** `move_center` clamped
//!     the *centre* to the screen rectangle, not the region around it, so at
//!     the corner three quarters of the view was sampled out of bounds and came
//!     back black. The source rectangle is now clamped as a rectangle.
//!
//! Two more things that were not bugs so much as absences: the app asked for no
//! clock, so the smooth-tracking the `smooth_edges` field promised could not
//! have eased anything (this is the seventh application of `known-issues.md`
//! lesson 47 — "an app that keeps time but never receives the clock"); and only
//! six of the ten zoom presets had a key, with 6x, 15x and 20x reachable solely
//! by stepping.
//!
//! ## The screen it magnifies
//!
//! There is no compositor capture API yet, so [`sample_pixel`] stands in for
//! the framebuffer read with a deterministic synthetic pattern. That is a
//! stub with a real interface: it takes a screen coordinate and returns a
//! colour, which is exactly what a capture call will, so replacing it touches
//! one function. Everything above it — the source-rectangle arithmetic, the
//! filters, the sampling grid, the colour readout — is the real thing and is
//! tested as such.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The screen this magnifier is pointed at, until there is an API to ask.
const SCREEN_WIDTH: f32 = 1920.0;
const SCREEN_HEIGHT: f32 = 1080.0;

/// How often the view is asked to ease toward what it is tracking.
///
/// A floor, not a promise: [`Magnifier::tick`] eases by the time the tick
/// reports, not by this constant, so a loop that ran late does not make the
/// pan run slow.
const TICK: Duration = Duration::from_millis(33);

/// The fraction of the remaining distance the smoothed view closes each
/// second. `0.92` reaches halfway in about 275 ms, which reads as "it followed
/// me" rather than either a jump or a drift.
const EASE_PER_SECOND: f32 = 0.92;

/// Under this many pixels, easing snaps. Without a floor the view creeps toward
/// its target forever by ever-smaller fractions and the app never stops asking
/// for frames.
const EASE_SNAP: f32 = 0.5;

/// One arrow key's worth of pan, in screen pixels; and with Ctrl held.
const PAN_STEP: f32 = 10.0;
const PAN_STEP_FAST: f32 = 50.0;

/// The lens is square-cornered or round, but always these bounds.
const LENS_MIN: f32 = 100.0;
const LENS_MAX: f32 = 800.0;
const LENS_STEP: f32 = 20.0;

/// The docked strip's share of the viewport, and how far `[` and `]` move it.
const DOCK_MIN: f32 = 0.10;
const DOCK_MAX: f32 = 0.80;
const DOCK_STEP: f32 = 0.05;

/// The most sample blocks drawn along one edge of a magnified pane.
///
/// The magnified view is a grid of blocks, one screen pixel each. At 1.5x in a
/// wide window that is over a thousand blocks across, and the frame is a `Vec`
/// of draw commands — a million rectangles per frame is not a picture, it is a
/// stall. Past this the grid coarsens: each block covers several screen pixels
/// and shows the one at its middle. The picture is then an honest downsample of
/// the region rather than a lie about the zoom.
const MAX_BLOCKS: usize = 128;

const HELP_TITLE: &str = "Keyboard shortcuts";

const HELP_ROWS: [(&str, &str); 14] = [
    ("Ctrl + / Ctrl -", "Zoom in / out"),
    ("Ctrl 0", "Back to 2x"),
    ("1 - 9, 0", "The ten zoom presets"),
    ("Arrows", "Pan (Ctrl for a bigger step)"),
    ("Shift arrows", "Resize the lens"),
    ("[ ]", "Shrink / grow the docked strip"),
    ("M / T / F", "Mode / tracking / colour filter"),
    ("X / L / G", "Crosshair / lens shape / smoothing"),
    ("C", "Read the colour under the crosshair"),
    ("R", "Ruler: start, finish, clear"),
    ("Ctrl S", "Save a shot of the magnified view"),
    ("Tab", "Hide the chrome"),
    ("Esc", "Pause / resume"),
    ("H or F1", "This sheet"),
];

// ── Magnification mode ─────────────────────────────────────────────────────

/// Where the magnified picture goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnifyMode {
    /// The whole viewport is the magnified region.
    FullScreen,
    /// A lens over a life-size view, positioned at the pointer.
    Lens,
    /// A strip along the top, life size below it.
    DockedTop,
    /// A strip along the bottom, life size above it.
    DockedBottom,
}

/// In the order `M` and the Mode button walk them.
pub const MODES: [MagnifyMode; 4] = [
    MagnifyMode::FullScreen,
    MagnifyMode::Lens,
    MagnifyMode::DockedTop,
    MagnifyMode::DockedBottom,
];

impl MagnifyMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full screen",
            Self::Lens => "Lens",
            Self::DockedTop => "Docked top",
            Self::DockedBottom => "Docked bottom",
        }
    }

    /// The next mode in [`MODES`], wrapping.
    ///
    /// Derived from the constant rather than written out a second time as a
    /// `match`: the old program had a hand-written `next` beside a hand-written
    /// order, which is two places for one order to live.
    #[must_use]
    pub fn next(self) -> Self {
        next_in(&MODES, self)
    }

    /// Whether this mode magnifies the whole viewport.
    #[must_use]
    pub fn is_full(self) -> bool {
        self == Self::FullScreen
    }

    /// Whether this mode puts a docked strip at the top.
    #[must_use]
    pub fn docks_top(self) -> bool {
        self == Self::DockedTop
    }
}

// ── Tracking ───────────────────────────────────────────────────────────────

/// What the magnified region follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingMode {
    /// The pointer, wherever it goes in the viewport.
    FollowMouse,
    /// Whatever last claimed keyboard focus — reported to us, not guessed.
    FollowFocus,
    /// Nothing; the arrow keys and clicks move it.
    Manual,
}

pub const TRACKINGS: [TrackingMode; 3] = [
    TrackingMode::FollowMouse,
    TrackingMode::FollowFocus,
    TrackingMode::Manual,
];

impl TrackingMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FollowMouse => "Follow mouse",
            Self::FollowFocus => "Follow focus",
            Self::Manual => "Manual",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        next_in(&TRACKINGS, self)
    }
}

/// The item after `current` in `all`, wrapping; the first if it is not there.
fn next_in<T: Copy + PartialEq>(all: &[T], current: T) -> T {
    let here = all.iter().position(|&x| x == current).unwrap_or(0);
    let then = here.saturating_add(1).checked_rem(all.len()).unwrap_or(0);
    all.get(then).copied().unwrap_or(current)
}

// ── Colour filters ─────────────────────────────────────────────────────────

/// What is done to every sampled pixel before it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFilter {
    None,
    Inverted,
    YellowOnBlack,
    WhiteOnBlack,
    GreenOnBlack,
    Greyscale,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

pub const FILTERS: [ColorFilter; 9] = [
    ColorFilter::None,
    ColorFilter::Inverted,
    ColorFilter::YellowOnBlack,
    ColorFilter::WhiteOnBlack,
    ColorFilter::GreenOnBlack,
    ColorFilter::Greyscale,
    ColorFilter::Protanopia,
    ColorFilter::Deuteranopia,
    ColorFilter::Tritanopia,
];

/// The luma above which a high-contrast filter calls a pixel light.
const CONTRAST_SPLIT: u8 = 128;

impl ColorFilter {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No filter",
            Self::Inverted => "Inverted",
            Self::YellowOnBlack => "Yellow on black",
            Self::WhiteOnBlack => "White on black",
            Self::GreenOnBlack => "Green on black",
            Self::Greyscale => "Greyscale",
            Self::Protanopia => "Protanopia",
            Self::Deuteranopia => "Deuteranopia",
            Self::Tritanopia => "Tritanopia",
        }
    }

    /// A name short enough for a button.
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Inverted => "Invert",
            Self::YellowOnBlack => "Y/Blk",
            Self::WhiteOnBlack => "W/Blk",
            Self::GreenOnBlack => "G/Blk",
            Self::Greyscale => "Grey",
            Self::Protanopia => "Prot",
            Self::Deuteranopia => "Deut",
            Self::Tritanopia => "Trit",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        next_in(&FILTERS, self)
    }

    /// The colour this filter shows for a screen pixel of `(r, g, b)`.
    #[must_use]
    pub fn apply(self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        match self {
            Self::None => (r, g, b),
            // Saturating, not wrapping. `255 - r` cannot overflow either way,
            // but `wrapping_sub` on a subtraction that is meant never to wrap
            // says the wrong thing about what the code expects.
            Self::Inverted => (
                u8::MAX.saturating_sub(r),
                u8::MAX.saturating_sub(g),
                u8::MAX.saturating_sub(b),
            ),
            Self::YellowOnBlack => Self::two_tone(r, g, b, (255, 255, 0)),
            Self::WhiteOnBlack => Self::two_tone(r, g, b, (255, 255, 255)),
            Self::GreenOnBlack => Self::two_tone(r, g, b, (0, 255, 0)),
            Self::Greyscale => {
                let l = Self::luma(r, g, b);
                (l, l, l)
            }
            // Brettel-style dichromacy approximations: each collapses the axis
            // the missing cone would have carried and leaves the other two.
            Self::Protanopia => Self::mix(
                r,
                g,
                b,
                [0.567, 0.433, 0.0],
                [0.558, 0.442, 0.0],
                [0.0, 0.242, 0.758],
            ),
            Self::Deuteranopia => Self::mix(
                r,
                g,
                b,
                [0.625, 0.375, 0.0],
                [0.7, 0.3, 0.0],
                [0.0, 0.3, 0.7],
            ),
            Self::Tritanopia => Self::mix(
                r,
                g,
                b,
                [0.95, 0.05, 0.0],
                [0.0, 0.433, 0.567],
                [0.0, 0.475, 0.525],
            ),
        }
    }

    /// `light` where the pixel is light, black where it is dark.
    fn two_tone(r: u8, g: u8, b: u8, light: (u8, u8, u8)) -> (u8, u8, u8) {
        if Self::luma(r, g, b) > CONTRAST_SPLIT {
            light
        } else {
            (0, 0, 0)
        }
    }

    /// Three rows of a 3x3 colour matrix, applied and clamped.
    ///
    /// Public because the clamping is the part worth testing and the matrices
    /// that ship all happen to have rows summing to one — so no test driven
    /// through [`apply`](Self::apply) alone can reach either end of the clamp.
    #[must_use]
    pub fn mix(
        r: u8,
        g: u8,
        b: u8,
        row_r: [f32; 3],
        row_g: [f32; 3],
        row_b: [f32; 3],
    ) -> (u8, u8, u8) {
        let v = [f32::from(r), f32::from(g), f32::from(b)];
        let dot = |row: [f32; 3]| -> u8 {
            let sum = row[0] * v[0] + row[1] * v[1] + row[2] * v[2];
            // Clamped at both ends. `min(255.0)` alone leaves a negative
            // coefficient free to produce a negative float, and `as u8` on a
            // negative float saturates to 0 by accident rather than by intent.
            sum.clamp(0.0, 255.0) as u8
        };
        (dot(row_r), dot(row_g), dot(row_b))
    }

    /// BT.601 luma. The three coefficients sum to exactly one, so white is 255.
    #[must_use]
    pub fn luma(r: u8, g: u8, b: u8) -> u8 {
        let l = f32::from(r) * 0.299 + f32::from(g) * 0.587 + f32::from(b) * 0.114;
        l.clamp(0.0, 255.0) as u8
    }
}

// ── Lens ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensShape {
    Circle,
    Rectangle,
}

impl LensShape {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Circle => "Round",
            Self::Rectangle => "Square",
        }
    }

    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Circle => Self::Rectangle,
            Self::Rectangle => Self::Circle,
        }
    }

    /// The corner radius this shape wants for a lens of the given size.
    #[must_use]
    pub fn radius(self, w: f32, h: f32) -> f32 {
        match self {
            Self::Circle => w.min(h).max(0.0) / 2.0,
            Self::Rectangle => 8.0,
        }
    }
}

// ── Zoom ───────────────────────────────────────────────────────────────────

/// Every zoom the program can be at, in order.
///
/// The zoom is *an index into this array* on the model, not a float that gets
/// snapped back to the nearest preset on every step. The old program stored the
/// float and recovered the index with a nearest-match search, so a zoom set to
/// anything between two presets forgot which side it came from: stepping up
/// from 2.4 and from 2.6 both landed on 3.0, one of which skipped a preset.
pub const ZOOM_PRESETS: [f32; 10] = [1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 15.0, 20.0];

/// The preset the program opens at and `Ctrl 0` returns to: 2x.
pub const DEFAULT_PRESET: usize = 1;

/// The zoom at `preset`, or the default if the index is out of range.
#[must_use]
pub fn zoom_of(preset: usize) -> f32 {
    ZOOM_PRESETS
        .get(preset)
        .copied()
        .or_else(|| ZOOM_PRESETS.get(DEFAULT_PRESET).copied())
        .unwrap_or(1.0)
}

// ── The screen being magnified ─────────────────────────────────────────────

/// The colour of one screen pixel.
///
/// A stub for a compositor capture that does not exist yet, with the interface
/// the real one will have: a screen coordinate in, a colour out. Out of bounds
/// is black, which is what a magnifier pointed off the edge of the display
/// should show — not a wrapped-around sample from the far side.
#[must_use]
pub fn sample_pixel(x: i32, y: i32, screen_w: i32, screen_h: i32) -> (u8, u8, u8) {
    if x < 0 || y < 0 || x >= screen_w || y >= screen_h {
        return (0, 0, 0);
    }
    let ux = x as u32;
    let uy = y as u32;
    let r = ux.wrapping_mul(7).wrapping_add(uy.wrapping_mul(13)) % 256;
    let g = ux.wrapping_mul(11).wrapping_add(uy.wrapping_mul(5)) % 256;
    let b = ux.wrapping_mul(3).wrapping_add(uy.wrapping_mul(17)) % 256;
    (r as u8, g as u8, b as u8)
}

// ── Ruler ──────────────────────────────────────────────────────────────────

/// Where the ruler is in its three-step life.
///
/// A state machine rather than the old `ruler_active: bool` plus
/// `ruler_measuring: bool` plus `ruler: Option<..>`, which is eight
/// combinations for three states and left five of them meaning nothing in
/// particular.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ruler {
    Off,
    /// One end is planted; the other follows the magnified centre.
    Measuring {
        start: (f32, f32),
        end: (f32, f32),
    },
    /// Both ends are planted.
    Done {
        start: (f32, f32),
        end: (f32, f32),
    },
}

impl Ruler {
    /// The two ends, if it has any.
    #[must_use]
    pub fn ends(self) -> Option<((f32, f32), (f32, f32))> {
        match self {
            Self::Off => None,
            Self::Measuring { start, end } | Self::Done { start, end } => Some((start, end)),
        }
    }

    /// The length between the ends **in screen pixels**, which is the unit its
    /// endpoints are in.
    #[must_use]
    pub fn screen_length(self) -> f32 {
        match self.ends() {
            None => 0.0,
            Some(((sx, sy), (ex, ey))) => {
                let dx = ex - sx;
                let dy = ey - sy;
                (dx * dx + dy * dy).sqrt()
            }
        }
    }

    /// How long that same span looks *on the magnified picture*.
    ///
    /// A multiplication. The old `screen_distance` divided, and the status line
    /// then reported the quotient as the magnified length — so at 10x a
    /// hundred-pixel span was announced as ten pixels long, a hundred times
    /// short of the thousand it actually covered.
    #[must_use]
    pub fn magnified_length(self, zoom: f32) -> f32 {
        self.screen_length() * zoom.max(0.0)
    }
}

// ── Screen-to-window geometry ──────────────────────────────────────────────

/// The region of the screen a magnified pane shows.
///
/// `pane` is a rectangle of the window; the result is the rectangle of the
/// screen scaled into it. It is `pane / zoom` in size, centred on `centre`, and
/// then **slid** — not shrunk — to lie inside the screen if it fits. Sliding is
/// what keeps a magnifier pointed at the corner of the display showing the
/// corner of the display rather than a quarter of a picture and three quarters
/// of the black that [`sample_pixel`] returns off the edge.
#[must_use]
pub fn source_rect(pane: Rect, zoom: f32, centre: (f32, f32), screen: (f32, f32)) -> Rect {
    let z = zoom.max(0.01);
    let w = (pane.w / z).max(0.0);
    let h = (pane.h / z).max(0.0);
    // Wider than the screen: there is nothing to slide into, so centre it on
    // the screen and let the margins be black on both sides rather than all of
    // it on one.
    let x = if w >= screen.0 {
        (screen.0 - w) / 2.0
    } else {
        (centre.0 - w / 2.0).clamp(0.0, screen.0 - w)
    };
    let y = if h >= screen.1 {
        (screen.1 - h) / 2.0
    } else {
        (centre.1 - h / 2.0).clamp(0.0, screen.1 - h)
    };
    Rect::new(x, y, w, h)
}

/// The screen point under a window point, for a pane showing `src`.
///
/// The exact inverse of the mapping [`source_rect`] sets up, so a click in the
/// magnified view names the pixel it landed on. Points outside the pane map
/// outside the source rectangle rather than being clamped: a caller that wants
/// to know a click missed should be able to see that it missed.
#[must_use]
pub fn screen_point(pane: Rect, src: Rect, x: f32, y: f32) -> (f32, f32) {
    let fx = if pane.w > 0.0 {
        (x - pane.x) / pane.w
    } else {
        0.0
    };
    let fy = if pane.h > 0.0 {
        (y - pane.y) / pane.h
    } else {
        0.0
    };
    (src.x + fx * src.w, src.y + fy * src.h)
}

/// The window point a screen point falls at, for a pane showing `src`.
#[must_use]
pub fn window_point(pane: Rect, src: Rect, sx: f32, sy: f32) -> (f32, f32) {
    let fx = if src.w > 0.0 {
        (sx - src.x) / src.w
    } else {
        0.0
    };
    let fy = if src.h > 0.0 {
        (sy - src.y) / src.h
    } else {
        0.0
    };
    (pane.x + fx * pane.w, pane.y + fy * pane.h)
}

// ── Targets and actions ────────────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The magnified pane. A click in it names a screen point.
    Magnified,
    /// The life-size pane beside a docked strip, or under the lens.
    LifeSize,
    ZoomOut,
    ZoomIn,
    NextMode,
    NextTracking,
    NextFilter,
    ToggleCrosshair,
    PickColour,
    ToggleRuler,
    TogglePause,
    ToggleHelp,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the magnifier can be asked to do, from either input.
///
/// One enum for both inputs, applied in one place, so a button and its key
/// cannot come to mean different things — which is what happened to the old
/// program's two key handlers, only one of which checked the modifiers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    ZoomIn,
    ZoomOut,
    /// Jump to `ZOOM_PRESETS[i]`.
    SetPreset(usize),
    /// Move the tracked point by this many screen pixels.
    Pan(f32, f32),
    /// Put the tracked point at this screen coordinate.
    JumpTo(f32, f32),
    NextMode,
    NextTracking,
    NextFilter,
    ToggleCrosshair,
    ToggleLensShape,
    ToggleSmooth,
    /// Change the lens by this many pixels in each direction.
    ResizeLens(f32, f32),
    /// Change the docked strip's share by this much.
    ResizeDock(f32),
    PickColour,
    /// Start, finish, or clear the ruler, in that cycle.
    StepRuler,
    Screenshot,
    ToggleChrome,
    TogglePause,
    ToggleHelp,
    CloseHelp,
}

impl Action {
    /// Whether this action is allowed while the magnifier is paused.
    ///
    /// Pausing used to stop only the drawing: `enabled` was read in `render`
    /// and nowhere else, so every key still worked and the effects piled up
    /// invisibly until you resumed. The three that still work are the ones that
    /// are *about* the paused state or about the window, not about the picture.
    #[must_use]
    pub fn allowed_while_paused(self) -> bool {
        matches!(
            self,
            Self::TogglePause | Self::ToggleHelp | Self::CloseHelp | Self::ToggleChrome
        )
    }
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the viewport keeps no matter what.
///
/// Higher than a game's would be, because this program *is* its viewport: the
/// chrome names the settings, and the picture is the whole point. Below this
/// share the chrome starts being dropped instead.
const VIEWPORT_SHARE: f32 = 0.62;

/// Which band goes first when they do not all fit: header, controls, info.
///
/// Whole bands, not a proportional squeeze: a band shrunk to four pixels costs
/// the viewport four pixels and shows nothing legible. The title goes first —
/// it names a program you are already looking at. The controls go next, because
/// every button has a key that still works without it. The info line goes last:
/// the zoom, the mode, the tracking, the filter and the colour readout are the
/// only chrome that says something the picture does not, and the colour readout
/// is the *entire output* of the colour picker.
const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];

/// The buttons of the control band, in the order they are drawn.
const CONTROLS: [(Target, &str); 10] = [
    (Target::ZoomOut, "\u{2212}"),
    (Target::ZoomIn, "+"),
    (Target::NextMode, "Mode"),
    (Target::NextTracking, "Track"),
    (Target::NextFilter, "Filter"),
    (Target::ToggleCrosshair, "Cross"),
    (Target::PickColour, "Pick"),
    (Target::ToggleRuler, "Ruler"),
    (Target::TogglePause, "Pause"),
    (Target::ToggleHelp, "Help"),
];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never stored on the model. A layout kept as
/// state is a layout that can disagree with the window it is drawn in — which
/// is exactly what a `width: 800.0, height: 600.0` pair assigned once in `new`
/// and read by every drawing function was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The name, the zoom and whether it is paused.
    pub header: Rect,
    /// Mode, tracking, filter, the ruler reading and the picked colour.
    pub info: Rect,
    /// Everything the viewport shows, magnified or not.
    pub viewport: Rect,
    /// The row of buttons.
    pub controls: Rect,
    pub help: Rect,
    pub font: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size.
    ///
    /// `chrome` is what Tab toggles: with it off there is nothing but the
    /// viewport, which is what someone actually reading their screen through
    /// this wants once the settings are right.
    #[must_use]
    pub fn new(width: f32, height: f32, chrome: bool) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 38.0).clamp(8.0, 17.0);
        let big = (font * 1.6).clamp(13.0, 28.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, controls] order.
        let mut wants = if chrome {
            [
                (h * 0.085).clamp(22.0, 44.0),
                (h * 0.06).clamp(16.0, 30.0),
                (h * 0.08).clamp(24.0, 42.0),
            ]
        } else {
            [0.0, 0.0, 0.0]
        };
        // What is left for chrome once the viewport has its share and the gaps
        // above and below it. The padding is charged here rather than to the
        // viewport, so a promised share of the window stays that share in a
        // small one.
        let budget = (h - h * VIEWPORT_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, ctl_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both read the same to `shows`, but only one reads the same to
        // anything asking "is this band gone, or merely thin?"
        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let info = if inf_h > 0.0 {
            Rect::new(0.0, hdr_h, w, inf_h)
        } else {
            Rect::EMPTY
        };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, h - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        // From the accumulated heights, not from `info.bottom()`. A dropped
        // band is `Rect::EMPTY`, whose bottom is zero, so reading the band back
        // would put the viewport over the header the moment the info line went
        // while the header stayed. `BAND_DROP_ORDER` drops the header first
        // today, so the two forms agree and no test can tell them apart — which
        // is the reason to write the safe one now rather than leave it to be
        // got right again by whoever reorders that constant.
        let top = hdr_h + inf_h;
        let bottom = if ctl_h > 0.0 { controls.y } else { h };
        let gap = if chrome { pad } else { 0.0 };
        let viewport = Rect::new(
            gap,
            top + gap,
            (w - gap * 2.0).max(0.0),
            (bottom - top - gap * 2.0).max(0.0),
        );

        // Bounded by construction, and left without a clamp on purpose. The
        // sheet is a *fraction* of the window before it is anything else, so
        // `w - help_w` is at least `0.06 * w` and the shortfall a centring
        // divides can never be negative — it scales with the window instead of
        // being a literal the window can undercut. A `.max(0.0)` here would be a
        // claim no input could reach, and this campaign's whole complaint about
        // unbounded centrings is that they make claims nothing checks; adding
        // one in the other direction is the same mistake mirrored.
        let help_w = (w * 0.94).min(460.0);
        let help_h = (h * 0.94).min(400.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            viewport,
            controls,
            help,
            font,
            big,
            pad,
        }
    }

    /// Whether a band is tall and wide enough for its text to be worth drawing.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.h >= 11.0 && band.w >= 110.0
    }

    /// The `slot`th of `count` buttons spread evenly across `band`.
    #[must_use]
    pub fn button(&self, band: Rect, slot: usize, count: usize) -> Rect {
        if count == 0 || slot >= count || band.w <= 0.0 || band.h <= 0.0 {
            return Rect::EMPTY;
        }
        let gap = self.pad * 0.4;
        let total = band.w - self.pad * 2.0;
        let each = ((total - gap * (count.saturating_sub(1)) as f32) / count as f32).max(0.0);
        // Bounded by construction: the button's height is the band's less a
        // constant, so `band.h - h` is `min(pad * 0.6, band.h)` — never
        // negative, and never more than the band. This is the one shape of
        // vertical centring that needs no `centre_line`, because the thing being
        // centred was derived by *subtracting* from the room rather than
        // measured independently of it.
        let h = (band.h - self.pad * 0.6).max(0.0);
        Rect::new(
            band.x + self.pad + slot as f32 * (each + gap),
            band.y + (band.h - h) / 2.0,
            each,
            h,
        )
    }

    /// The magnified pane and the life-size one, for a mode and a dock share.
    ///
    /// Both are cut out of the viewport, so neither can be drawn where the
    /// chrome is and neither can drift from the other. In full-screen mode the
    /// life-size pane is empty; in lens mode the *life-size* pane is the whole
    /// viewport and the lens is placed over it separately, because the lens
    /// moves with the pointer and is not a fixed division of anything.
    #[must_use]
    pub fn panes(&self, mode: MagnifyMode, dock: f32) -> (Rect, Rect) {
        let v = self.viewport;
        if v.w <= 0.0 || v.h <= 0.0 {
            return (Rect::EMPTY, Rect::EMPTY);
        }
        match mode {
            MagnifyMode::FullScreen => (v, Rect::EMPTY),
            MagnifyMode::Lens => (Rect::EMPTY, v),
            MagnifyMode::DockedTop | MagnifyMode::DockedBottom => {
                let strip = (v.h * dock.clamp(DOCK_MIN, DOCK_MAX)).max(0.0);
                let rest = (v.h - strip).max(0.0);
                if mode.docks_top() {
                    (
                        Rect::new(v.x, v.y, v.w, strip),
                        Rect::new(v.x, v.y + strip, v.w, rest),
                    )
                } else {
                    (
                        Rect::new(v.x, v.y + rest, v.w, strip),
                        Rect::new(v.x, v.y, v.w, rest),
                    )
                }
            }
        }
    }

    /// The lens rectangle, centred on a window point and kept inside the
    /// viewport.
    ///
    /// Kept inside because a lens half off the edge shows half a picture, and
    /// because the part of it that is off the edge is still a hit box: a click
    /// there would have been read against a pane the window never drew.
    #[must_use]
    pub fn lens(&self, at: (f32, f32), lens_w: f32, lens_h: f32) -> Rect {
        let v = self.viewport;
        let w = lens_w.min(v.w).max(0.0);
        let h = lens_h.min(v.h).max(0.0);
        let x = (at.0 - w / 2.0).clamp(v.x, (v.right() - w).max(v.x));
        let y = (at.1 - h / 2.0).clamp(v.y, (v.bottom() - h).max(v.y));
        Rect::new(x, y, w, h)
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

/// The magnifier.
pub struct Magnifier {
    /// An index into [`ZOOM_PRESETS`], not a float to be snapped back.
    preset: usize,
    mode: MagnifyMode,
    tracking: TrackingMode,
    filter: ColorFilter,

    /// The screen point the picture is centred on right now.
    centre: (f32, f32),
    /// The screen point it is heading for. Equal to `centre` unless smoothing
    /// is on and a tick is still owed.
    target: (f32, f32),

    /// The pointer's last position, in **window** coordinates.
    pointer: (f32, f32),

    paused: bool,
    crosshair: bool,
    smooth: bool,
    chrome: bool,
    show_help: bool,

    lens_shape: LensShape,
    lens_w: f32,
    lens_h: f32,
    dock: f32,

    ruler: Ruler,
    picked: Option<(u8, u8, u8)>,
    shots: u32,
    status: String,

    /// The screen being magnified.
    screen: (f32, f32),
    /// The size the window was last drawn at — what the next click is read
    /// against.
    size_drawn: (f32, f32),
}

impl Magnifier {
    #[must_use]
    pub fn new() -> Self {
        Self::with_screen(SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    /// A magnifier pointed at a screen of the given size.
    #[must_use]
    pub fn with_screen(screen_w: f32, screen_h: f32) -> Self {
        let screen = (screen_w.max(1.0), screen_h.max(1.0));
        let middle = (screen.0 / 2.0, screen.1 / 2.0);
        Self {
            preset: DEFAULT_PRESET,
            mode: MagnifyMode::FullScreen,
            tracking: TrackingMode::FollowMouse,
            filter: ColorFilter::None,
            centre: middle,
            target: middle,
            pointer: (WINDOW_WIDTH / 2.0, WINDOW_HEIGHT / 2.0),
            paused: false,
            crosshair: true,
            smooth: true,
            chrome: true,
            show_help: false,
            lens_shape: LensShape::Circle,
            lens_w: 300.0,
            lens_h: 300.0,
            dock: 0.33,
            ruler: Ruler::Off,
            picked: None,
            shots: 0,
            status: "Ready — Ctrl + and Ctrl - to zoom, H for the shortcuts".to_string(),
            screen,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    // ── Reading the state ──────────────────────────────────────────────────

    #[must_use]
    pub fn zoom(&self) -> f32 {
        zoom_of(self.preset)
    }

    #[must_use]
    pub fn preset(&self) -> usize {
        self.preset
    }

    #[must_use]
    pub fn mode(&self) -> MagnifyMode {
        self.mode
    }

    #[must_use]
    pub fn tracking(&self) -> TrackingMode {
        self.tracking
    }

    #[must_use]
    pub fn filter(&self) -> ColorFilter {
        self.filter
    }

    /// Where the picture is centred now.
    #[must_use]
    pub fn centre(&self) -> (f32, f32) {
        self.centre
    }

    /// Where it is heading. The same as [`Self::centre`] once it has arrived.
    #[must_use]
    pub fn target(&self) -> (f32, f32) {
        self.target
    }

    /// The last window point the pointer was seen at.
    ///
    /// Recorded under every tracking mode, not only the one that follows it —
    /// a test that means to prove the lens is *not* at the pointer needs to be
    /// able to say where the pointer is.
    #[must_use]
    pub fn pointer(&self) -> (f32, f32) {
        self.pointer
    }

    #[must_use]
    pub fn paused(&self) -> bool {
        self.paused
    }

    #[must_use]
    pub fn crosshair(&self) -> bool {
        self.crosshair
    }

    #[must_use]
    pub fn smooth(&self) -> bool {
        self.smooth
    }

    #[must_use]
    pub fn chrome(&self) -> bool {
        self.chrome
    }

    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    #[must_use]
    pub fn lens_shape(&self) -> LensShape {
        self.lens_shape
    }

    #[must_use]
    pub fn lens_size(&self) -> (f32, f32) {
        (self.lens_w, self.lens_h)
    }

    #[must_use]
    pub fn dock(&self) -> f32 {
        self.dock
    }

    #[must_use]
    pub fn ruler(&self) -> Ruler {
        self.ruler
    }

    #[must_use]
    pub fn picked(&self) -> Option<(u8, u8, u8)> {
        self.picked
    }

    #[must_use]
    pub fn shots(&self) -> u32 {
        self.shots
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    #[must_use]
    pub fn screen(&self) -> (f32, f32) {
        self.screen
    }

    /// Whether the view still has ground to cover toward its target.
    ///
    /// What [`App::tick_interval`] answers from, so the window is asked for a
    /// clock exactly while there is something for it to move and never
    /// otherwise.
    #[must_use]
    pub fn easing(&self) -> bool {
        let dx = self.target.0 - self.centre.0;
        let dy = self.target.1 - self.centre.1;
        dx.abs() > EASE_SNAP || dy.abs() > EASE_SNAP
    }

    /// The layout for the size the window was last drawn at.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1, self.chrome)
    }

    /// The magnified and life-size panes, at the size last drawn.
    #[must_use]
    pub fn panes(&self) -> (Rect, Rect) {
        self.layout().panes(self.mode, self.dock)
    }

    /// The lens rectangle, at the size last drawn.
    ///
    /// Placed over the window point that the centre falls at in the life-size
    /// pane, so the lens is always *over what it shows*. The old program put
    /// the lens at the pointer and filled it from the centre, which are the
    /// same number only while tracking follows the mouse — in manual tracking
    /// the lens sat under your hand and showed somewhere else.
    #[must_use]
    pub fn lens_rect(&self) -> Rect {
        self.lens_in(&self.layout())
    }

    /// The lens rectangle for a layout already in hand.
    ///
    /// One body, called by [`lens_rect`](Self::lens_rect) and by the drawing
    /// pass. Written twice it was written twice *differently* the first time —
    /// which is precisely fault eight — so there is one now and the drawing,
    /// the hit box and the reading all come from it.
    fn lens_in(&self, l: &Layout) -> Rect {
        let (_, life) = l.panes(self.mode, self.dock);
        let src = source_rect(life, 1.0, self.centre, self.screen);
        let at = window_point(life, src, self.centre.0, self.centre.1);
        l.lens(at, self.lens_w, self.lens_h)
    }

    /// The region of the screen the magnified pane is showing.
    #[must_use]
    pub fn magnified_source(&self) -> Rect {
        let (mag, _) = self.panes();
        let pane = if self.mode == MagnifyMode::Lens {
            self.lens_rect()
        } else {
            mag
        };
        source_rect(pane, self.zoom(), self.centre, self.screen)
    }

    // ── Changing the state ─────────────────────────────────────────────────

    /// Point the view at a screen coordinate.
    ///
    /// The target moves at once; the centre follows it over the next few ticks
    /// when smoothing is on, and jumps with it when it is off. Every route to
    /// a new centre goes through here, so "smoothing applies" is one fact in
    /// one place rather than a decision each caller could get differently.
    fn aim_at(&mut self, x: f32, y: f32) {
        self.target = (x.clamp(0.0, self.screen.0), y.clamp(0.0, self.screen.1));
        if !self.smooth {
            self.centre = self.target;
        }
    }

    fn set_preset(&mut self, i: usize) {
        let Some(&zoom) = ZOOM_PRESETS.get(i) else {
            return;
        };
        self.preset = i;
        self.status = format!("Zoom {}", trim_zoom(zoom));
    }

    /// Read the colour of a screen pixel, as the filter would show it.
    ///
    /// The *filtered* colour, deliberately: this readout is here so someone who
    /// cannot make out a colour can be told what it is, and what they are
    /// looking at is the filtered picture. The unfiltered value is in the
    /// status line beside it, so neither is hidden.
    fn pick_colour_at(&mut self, sx: f32, sy: f32) {
        let (r, g, b) = sample_pixel(
            sx as i32,
            sy as i32,
            self.screen.0 as i32,
            self.screen.1 as i32,
        );
        let shown = self.filter.apply(r, g, b);
        self.picked = Some(shown);
        self.status = format!(
            "{} at {:.0}, {:.0}{}",
            hex_of(shown),
            sx,
            sy,
            if self.filter == ColorFilter::None {
                String::new()
            } else {
                format!(" (screen {})", hex_of((r, g, b)))
            }
        );
    }

    /// Start the ruler, finish it, or clear it — in that cycle.
    fn step_ruler(&mut self) {
        self.ruler = match self.ruler {
            Ruler::Off => {
                self.status = "Ruler: move to the other end, then R again".to_string();
                Ruler::Measuring {
                    start: self.centre,
                    end: self.centre,
                }
            }
            Ruler::Measuring { start, .. } => {
                let done = Ruler::Done {
                    start,
                    end: self.centre,
                };
                self.status = format!(
                    "Ruler: {:.0} screen px, {:.0} px as shown",
                    done.screen_length(),
                    done.magnified_length(self.zoom())
                );
                done
            }
            Ruler::Done { .. } => {
                self.status = "Ruler cleared".to_string();
                Ruler::Off
            }
        };
    }

    /// The one place an action changes the magnifier.
    pub fn apply(&mut self, action: Action) {
        // The pause gate lives here rather than in the key handler, so a button
        // and its key are stopped by the same line. The old program's pause was
        // read only by the drawing code, so every key still worked while
        // "paused" and you found out what you had done when you resumed.
        if self.paused && !action.allowed_while_paused() {
            self.status = "Paused — Esc to resume".to_string();
            return;
        }
        match action {
            Action::ZoomIn => {
                self.set_preset(
                    self.preset
                        .saturating_add(1)
                        .min(ZOOM_PRESETS.len().saturating_sub(1)),
                );
            }
            Action::ZoomOut => self.set_preset(self.preset.saturating_sub(1)),
            Action::SetPreset(i) => self.set_preset(i),
            Action::Pan(dx, dy) => {
                // From the *target*, not the centre. Panning from the centre
                // while a smoothed pan is still running would throw away the
                // part of the last one that had not arrived yet, so holding an
                // arrow key would crawl instead of moving.
                let (tx, ty) = self.target;
                self.aim_at(tx + dx, ty + dy);
            }
            Action::JumpTo(x, y) => self.aim_at(x, y),
            Action::NextMode => {
                self.mode = self.mode.next();
                self.status = format!("Mode: {}", self.mode.label());
            }
            Action::NextTracking => {
                self.tracking = self.tracking.next();
                self.status = format!("Tracking: {}", self.tracking.label());
            }
            Action::NextFilter => {
                self.filter = self.filter.next();
                self.status = format!("Filter: {}", self.filter.label());
            }
            Action::ToggleCrosshair => {
                self.crosshair = !self.crosshair;
                self.status = if self.crosshair {
                    "Crosshair on".to_string()
                } else {
                    "Crosshair off".to_string()
                };
            }
            Action::ToggleLensShape => {
                self.lens_shape = self.lens_shape.toggle();
                self.status = format!("Lens: {}", self.lens_shape.label());
            }
            Action::ToggleSmooth => {
                self.smooth = !self.smooth;
                // Arriving at once when smoothing is switched off, rather than
                // leaving the view stranded part-way through a pan it will now
                // never finish because nothing will ask for another tick.
                if !self.smooth {
                    self.centre = self.target;
                }
                self.status = if self.smooth {
                    "Smooth tracking on".to_string()
                } else {
                    "Smooth tracking off".to_string()
                };
            }
            Action::ResizeLens(dw, dh) => {
                self.lens_w = (self.lens_w + dw).clamp(LENS_MIN, LENS_MAX);
                self.lens_h = (self.lens_h + dh).clamp(LENS_MIN, LENS_MAX);
                self.status = format!("Lens {:.0} x {:.0}", self.lens_w, self.lens_h);
            }
            Action::ResizeDock(d) => {
                self.dock = (self.dock + d).clamp(DOCK_MIN, DOCK_MAX);
                self.status = format!("Docked strip {:.0}%", self.dock * 100.0);
            }
            Action::PickColour => {
                let (x, y) = self.centre;
                self.pick_colour_at(x, y);
            }
            Action::StepRuler => self.step_ruler(),
            Action::Screenshot => {
                self.shots = self.shots.saturating_add(1);
                self.status = format!("Saved shot {}", self.shots);
            }
            Action::ToggleChrome => {
                self.chrome = !self.chrome;
                self.status = if self.chrome {
                    "Chrome shown".to_string()
                } else {
                    "Chrome hidden — Tab to bring it back".to_string()
                };
            }
            Action::TogglePause => {
                self.paused = !self.paused;
                self.status = if self.paused {
                    "Paused — Esc to resume".to_string()
                } else {
                    "Resumed".to_string()
                };
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.status = HELP_TITLE.to_string();
                }
            }
            Action::CloseHelp => {
                if self.show_help {
                    self.show_help = false;
                    self.status = "Ready".to_string();
                }
            }
        }
    }

    /// Ease the view toward its target by the time a tick says has passed.
    ///
    /// By `elapsed_ms`, never by [`TICK`]: the interval asked for is a floor,
    /// not a promise, so a pan driven by the constant runs slow by however much
    /// the loop was busy — and runs slow silently.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        if !self.easing() {
            // Land exactly, so `easing` cannot answer true forever on a
            // fraction of a pixel that never quite closes.
            self.centre = self.target;
            return EventResult::Ignored;
        }
        let seconds = elapsed_ms as f32 / 1000.0;
        // The fraction of the gap left after `seconds` of closing at
        // `EASE_PER_SECOND` per second. Framed as a decay rather than a
        // per-tick step so that the pan takes the same wall-clock time whether
        // the ticks arrive every 33 ms or every 200 ms.
        let remaining = (1.0 - EASE_PER_SECOND).powf(seconds.max(0.0));
        self.centre = (
            self.target.0 - (self.target.0 - self.centre.0) * remaining,
            self.target.1 - (self.target.1 - self.centre.1) * remaining,
        );
        if !self.easing() {
            self.centre = self.target;
        }
        EventResult::Consumed
    }

    // ── Input ──────────────────────────────────────────────────────────────

    /// What a key means, or nothing.
    ///
    /// One table for both states and both modifier sets. The old program had
    /// the modifier test on some arms and not others, and put the four
    /// shift-arrow arms *below* the four plain-arrow ones — which have `!ctrl`,
    /// not `!shift`, so the earlier arms swallowed every shift-arrow and the
    /// lens could not be resized by keyboard at all.
    #[must_use]
    pub fn key_action(&self, ev: &KeyEvent) -> Option<Action> {
        // A key coming back up is not a second press.
        if !ev.pressed {
            return None;
        }
        // Nothing in this program is bound to Alt or to the super key, so a
        // combination that uses them belongs to whatever else is listening.
        if ev.modifiers.alt || ev.modifiers.super_key {
            return None;
        }
        if self.show_help {
            return match ev.key {
                Key::Escape | Key::H | Key::F1 | Key::Enter | Key::Space => Some(Action::CloseHelp),
                _ => None,
            };
        }
        if ev.modifiers.ctrl {
            return match ev.key {
                Key::Equals => Some(Action::ZoomIn),
                Key::Minus => Some(Action::ZoomOut),
                Key::Num0 => Some(Action::SetPreset(DEFAULT_PRESET)),
                Key::S => Some(Action::Screenshot),
                Key::Left => Some(Action::Pan(-PAN_STEP_FAST, 0.0)),
                Key::Right => Some(Action::Pan(PAN_STEP_FAST, 0.0)),
                Key::Up => Some(Action::Pan(0.0, -PAN_STEP_FAST)),
                Key::Down => Some(Action::Pan(0.0, PAN_STEP_FAST)),
                _ => None,
            };
        }
        if ev.modifiers.shift {
            return match ev.key {
                Key::Left => Some(Action::ResizeLens(-LENS_STEP, 0.0)),
                Key::Right => Some(Action::ResizeLens(LENS_STEP, 0.0)),
                Key::Up => Some(Action::ResizeLens(0.0, -LENS_STEP)),
                Key::Down => Some(Action::ResizeLens(0.0, LENS_STEP)),
                _ => None,
            };
        }
        match ev.key {
            Key::Left => Some(Action::Pan(-PAN_STEP, 0.0)),
            Key::Right => Some(Action::Pan(PAN_STEP, 0.0)),
            Key::Up => Some(Action::Pan(0.0, -PAN_STEP)),
            Key::Down => Some(Action::Pan(0.0, PAN_STEP)),
            Key::M => Some(Action::NextMode),
            Key::T => Some(Action::NextTracking),
            Key::F => Some(Action::NextFilter),
            Key::X => Some(Action::ToggleCrosshair),
            Key::L => Some(Action::ToggleLensShape),
            Key::G => Some(Action::ToggleSmooth),
            Key::C => Some(Action::PickColour),
            Key::R => Some(Action::StepRuler),
            Key::LeftBracket => Some(Action::ResizeDock(-DOCK_STEP)),
            Key::RightBracket => Some(Action::ResizeDock(DOCK_STEP)),
            Key::Tab => Some(Action::ToggleChrome),
            Key::Escape => Some(Action::TogglePause),
            Key::H | Key::F1 => Some(Action::ToggleHelp),
            // All ten presets, not the six that had keys before: 6x, 15x and
            // 20x could only be reached by stepping through the ones below.
            Key::Num1 => Some(Action::SetPreset(0)),
            Key::Num2 => Some(Action::SetPreset(1)),
            Key::Num3 => Some(Action::SetPreset(2)),
            Key::Num4 => Some(Action::SetPreset(3)),
            Key::Num5 => Some(Action::SetPreset(4)),
            Key::Num6 => Some(Action::SetPreset(5)),
            Key::Num7 => Some(Action::SetPreset(6)),
            Key::Num8 => Some(Action::SetPreset(7)),
            Key::Num9 => Some(Action::SetPreset(8)),
            Key::Num0 => Some(Action::SetPreset(9)),
            _ => None,
        }
    }

    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        match self.key_action(ev) {
            Some(action) => {
                self.apply(action);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// The target under a window point, read from the frame last drawn.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.size_drawn.0, self.size_drawn.1)
            .hit_test(x, y)
    }

    /// The screen point a window point falls on, for whichever pane it is in.
    ///
    /// `None` when it is not over either pane. Reading a click's screen
    /// coordinate has to know which pane it landed in, because the two are at
    /// different scales — a click in the docked strip is at the zoom and a
    /// click below it is at life size.
    #[must_use]
    pub fn screen_at(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (mag, life) = self.panes();
        if self.mode == MagnifyMode::Lens {
            let lens = self.lens_rect();
            if lens.contains(x, y) {
                let src = source_rect(lens, self.zoom(), self.centre, self.screen);
                return Some(screen_point(lens, src, x, y));
            }
        }
        if mag.contains(x, y) {
            let src = source_rect(mag, self.zoom(), self.centre, self.screen);
            return Some(screen_point(mag, src, x, y));
        }
        if life.contains(x, y) {
            let src = source_rect(life, 1.0, self.centre, self.screen);
            return Some(screen_point(life, src, x, y));
        }
        None
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        match ev.kind {
            MouseEventKind::Move => {
                self.pointer = (ev.x, ev.y);
                // Motion re-aims only under `FollowMouse`. Under `FollowFocus`
                // and `Manual` the pointer is just a pointer, which is the
                // whole difference between the three settings.
                if self.paused || self.show_help || self.tracking != TrackingMode::FollowMouse {
                    return EventResult::Ignored;
                }
                let Some((sx, sy)) = self.screen_at(ev.x, ev.y) else {
                    return EventResult::Ignored;
                };
                self.apply(Action::JumpTo(sx, sy));
                EventResult::Consumed
            }
            MouseEventKind::Press(MouseButton::Left) => {
                self.pointer = (ev.x, ev.y);
                // No special case for the open sheet: `draw_help` records one
                // hit box over the whole window, last, and `hit_test` takes the
                // last box at a point — so the ordinary lookup already answers
                // `ToggleHelp` everywhere while the sheet is up.
                let Some(target) = self.target_at(ev.x, ev.y) else {
                    return EventResult::Ignored;
                };
                self.press(target, ev.x, ev.y)
            }
            _ => EventResult::Ignored,
        }
    }

    /// What a click on a target does.
    fn press(&mut self, target: Target, x: f32, y: f32) -> EventResult {
        match target {
            Target::Magnified | Target::LifeSize => {
                // A click is somewhere you meant to look, so it aims the view
                // under every tracking mode but `Manual` — which is what
                // `Manual` means.
                if self.tracking == TrackingMode::Manual {
                    return EventResult::Ignored;
                }
                let Some((sx, sy)) = self.screen_at(x, y) else {
                    return EventResult::Ignored;
                };
                self.apply(Action::JumpTo(sx, sy));
            }
            Target::ZoomIn => self.apply(Action::ZoomIn),
            Target::ZoomOut => self.apply(Action::ZoomOut),
            Target::NextMode => self.apply(Action::NextMode),
            Target::NextTracking => self.apply(Action::NextTracking),
            Target::NextFilter => self.apply(Action::NextFilter),
            Target::ToggleCrosshair => self.apply(Action::ToggleCrosshair),
            Target::PickColour => self.apply(Action::PickColour),
            Target::ToggleRuler => self.apply(Action::StepRuler),
            Target::TogglePause => self.apply(Action::TogglePause),
            Target::ToggleHelp => self.apply(Action::ToggleHelp),
        }
        EventResult::Consumed
    }

    /// Remember the size the window is drawn at, so the next click is read
    /// against the pixels the user is looking at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    /// What the info band says.
    ///
    /// Split out so a test can read it without hunting a substring through the
    /// frame's text commands.
    #[must_use]
    pub fn info_line(&self) -> String {
        let colour = match self.picked {
            Some(rgb) => hex_of(rgb),
            None => "no pick".to_string(),
        };
        let ruler = match self.ruler {
            Ruler::Off => "no ruler".to_string(),
            Ruler::Measuring { .. } => "ruler open".to_string(),
            Ruler::Done { .. } => format!("{:.0} px", self.ruler.screen_length()),
        };
        format!(
            "{}  \u{2022}  {}  \u{2022}  {}  \u{2022}  {colour}  \u{2022}  {ruler}  \u{2022}  {} shot{}",
            self.mode.label(),
            self.tracking.label(),
            self.filter.label(),
            self.shots,
            if self.shots == 1 { "" } else { "s" },
        )
    }
}

impl Default for Magnifier {
    fn default() -> Self {
        Self::new()
    }
}

/// `2` rather than `2.0`, but `1.5` still `1.5`.
fn trim_zoom(zoom: f32) -> String {
    if (zoom - zoom.round()).abs() < 0.05 {
        format!("{:.0}x", zoom)
    } else {
        format!("{:.1}x", zoom)
    }
}

fn hex_of((r, g, b): (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

// ── Drawing ────────────────────────────────────────────────────────────────

/// How many sample blocks to draw along an edge showing `source_span` screen
/// pixels.
///
/// One per screen pixel until that would be too many to draw, then as many as
/// [`MAX_BLOCKS`] allows and each block covers several.
#[must_use]
pub fn block_count(source_span: f32) -> usize {
    let wanted = source_span.ceil().max(1.0);
    (wanted as usize).clamp(1, MAX_BLOCKS)
}

impl Magnifier {
    /// The frame for a window of the given size, hit boxes and all.
    ///
    /// The drawing pass is what records the hit boxes, so a button is clickable
    /// exactly where it was drawn and the two cannot drift apart. The old
    /// program recorded none at all — it drew `[H]elp [M]ode [T]rack [F]ilter`
    /// into the toolbar as text and had no mouse handler to make any of it mean
    /// anything.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height, self.chrome);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_viewport(&mut f, &l);
        self.draw_header(&mut f, &l);
        self.draw_info(&mut f, &l);
        self.draw_controls(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    // ── The picture ────────────────────────────────────────────────────────

    fn draw_viewport(&self, f: &mut Frame, l: &Layout) {
        let v = l.viewport;
        if v.w <= 0.0 || v.h <= 0.0 {
            return;
        }
        fill(f, v, CRUST, 0.0);
        if self.paused {
            self.draw_paused(f, l);
            return;
        }
        let (mag, life) = l.panes(self.mode, self.dock);

        // The life-size pane first, so that in lens mode the lens's hit box is
        // recorded after it and therefore wins the lookup.
        if life.w > 0.0 && life.h > 0.0 {
            self.draw_pane(f, life, 1.0);
            f.hit(Target::LifeSize, life);
        }
        if mag.w > 0.0 && mag.h > 0.0 {
            self.draw_pane(f, mag, self.zoom());
            f.hit(Target::Magnified, mag);
            self.draw_edge(f, mag);
        }
        if self.mode == MagnifyMode::Lens {
            self.draw_lens(f, l);
        }

        let pane = self.showing_pane(l);
        if pane.w > 0.0 && pane.h > 0.0 {
            if self.crosshair {
                self.draw_crosshair(f, pane);
            }
            self.draw_ruler(f, pane);
        }
    }

    /// The pane the magnified picture is actually in — the lens in lens mode,
    /// the magnified pane otherwise.
    fn showing_pane(&self, l: &Layout) -> Rect {
        if self.mode == MagnifyMode::Lens {
            self.lens_in(l)
        } else {
            l.panes(self.mode, self.dock).0
        }
    }

    /// One pane of screen content, sampled and filtered.
    fn draw_pane(&self, f: &mut Frame, pane: Rect, zoom: f32) {
        let src = source_rect(pane, zoom, self.centre, self.screen);
        let cols = block_count(src.w);
        let rows = block_count(src.h);
        let bw = pane.w / cols as f32;
        let bh = pane.h / rows as f32;
        let sw = self.screen.0 as i32;
        let sh = self.screen.1 as i32;
        f.clip(pane);
        for row in 0..rows {
            for col in 0..cols {
                // The middle of the block's slice of the source, so a block
                // that covers several screen pixels shows a representative one
                // rather than the one on its edge.
                let sx = src.x + (col as f32 + 0.5) * src.w / cols as f32;
                let sy = src.y + (row as f32 + 0.5) * src.h / rows as f32;
                let (r, g, b) = sample_pixel(sx as i32, sy as i32, sw, sh);
                let (fr, fg, fb) = self.filter.apply(r, g, b);
                let bx = pane.x + col as f32 * bw;
                let by = pane.y + row as f32 * bh;
                f.push(RenderCommand::FillRect {
                    x: bx,
                    y: by,
                    // A hair over, so that rounding between adjacent blocks
                    // cannot leave a background-coloured seam between them —
                    // but cut to the pane, because the last block of each row
                    // and column has no neighbour to cover and spends its half
                    // pixel outside the pane instead. The clip below hid that,
                    // which is both why it went unnoticed and why it is worth
                    // fixing here rather than leaving to the clip: a block the
                    // clip has to cut is a block drawn where it does not
                    // belong, and no test that reads the commands can tell.
                    // Every interior seam is still covered, because each is
                    // covered by the block *before* it.
                    width: (bw + 0.5).min(pane.right() - bx),
                    height: (bh + 0.5).min(pane.bottom() - by),
                    color: Color::rgb(fr, fg, fb),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        f.unclip();
    }

    /// The line that says where the magnified strip stops.
    fn draw_edge(&self, f: &mut Frame, mag: Rect) {
        if self.mode.is_full() || self.mode == MagnifyMode::Lens {
            return;
        }
        let y = if self.mode.docks_top() {
            mag.bottom()
        } else {
            mag.y
        };
        line(f, mag.x, y, mag.right(), y, BLUE, 2.0);
    }

    fn draw_lens(&self, f: &mut Frame, l: &Layout) {
        let lens = self.showing_pane(l);
        if lens.w <= 0.0 || lens.h <= 0.0 {
            return;
        }
        let radius = self.lens_shape.radius(lens.w, lens.h);
        fill(f, lens, MANTLE, radius);
        self.draw_pane(f, lens, self.zoom());
        stroke(f, lens, BLUE, 2.0, radius);
        // After the life-size pane's box, so a click inside the lens is a click
        // on the magnified view and reads at the magnified scale.
        f.hit(Target::Magnified, lens);
    }

    fn draw_crosshair(&self, f: &mut Frame, pane: Rect) {
        let src = source_rect(pane, self.zoom(), self.centre, self.screen);
        let (cx, cy) = window_point(pane, src, self.centre.0, self.centre.1);
        let arm = (pane.w.min(pane.h) * 0.06).clamp(6.0, 24.0);
        f.clip(pane);
        line(f, cx - arm, cy, cx + arm, cy, RED, 2.0);
        line(f, cx, cy - arm, cx, cy + arm, RED, 2.0);
        f.unclip();
    }

    fn draw_ruler(&self, f: &mut Frame, pane: Rect) {
        let Some((start, end)) = self.ruler.ends() else {
            return;
        };
        let src = source_rect(pane, self.zoom(), self.centre, self.screen);
        let (x1, y1) = window_point(pane, src, start.0, start.1);
        let (x2, y2) = window_point(pane, src, end.0, end.1);
        f.clip(pane);
        // A line between the two ends, not a horizontal bar between their two
        // x coordinates: the old drawing took `min_x`, `max_x` and `min_y` and
        // never looked at `end_y`, so every measurement was drawn flat however
        // it had been taken.
        line(f, x1, y1, x2, y2, YELLOW, 2.0);
        let tick = 5.0;
        line(f, x1, y1 - tick, x1, y1 + tick, YELLOW, 2.0);
        line(f, x2, y2 - tick, x2, y2 + tick, YELLOW, 2.0);
        let text_str = format!("{:.0} px", self.ruler.screen_length());
        let size = (pane.h * 0.05).clamp(8.0, 13.0);
        let w = text::measure(&text_str, size, FontWeightHint::Bold);
        // The reading is cut to the pane rather than left to the clip.
        //
        // Its box floats with the measurement — centred between the two ends
        // and a line above them — so all four of its edges can leave the pane,
        // and every one of them did: `Some(pane.w)` as a limit reaches the
        // pane's full *width* from wherever the reading happens to start, and
        // the `- line_h - 2.0` puts it above the pane whenever the measurement
        // is taken near the top. The clip hid all of it. That is the worst
        // shape a bound can have: the drawing is wrong, the picture is right,
        // and no test that reads the commands can tell. `intersect` gives the
        // four bounds and the refusal in one, and the clip stays for the line
        // and its ticks, which genuinely want to be cut off rather than moved.
        let line_h = text::line_height(size, FontWeightHint::Bold);
        if let Some(box_) = Rect::new(
            f32::midpoint(x1, x2) - w / 2.0,
            f32::midpoint(y1, y2) - line_h - 2.0,
            w,
            line_h,
        )
        .intersect(pane)
        {
            push_text(
                f,
                box_.x,
                box_.y,
                &text_str,
                size,
                YELLOW,
                FontWeightHint::Bold,
                box_.right() - box_.x,
            );
        }
        f.unclip();
    }

    fn draw_paused(&self, f: &mut Frame, l: &Layout) {
        let v = l.viewport;
        let size = l.big.min(v.h * 0.2);
        let line_h = text::line_height(size, FontWeightHint::Bold);
        let small = (size * 0.6).max(8.0);
        let small_h = text::line_height(small, FontWeightHint::Regular);
        // Both lines used to be placed by offsetting *from* a centring — one at
        // `- line_h` and one at `+ line_h * 0.4` — which is worse than a bare
        // centring rather than better. A centring is unbounded by one half of
        // the overhang; a centring plus a further constant is unbounded by that
        // and by the constant, and no amount of viewport is enough to make the
        // second term safe because it does not scale with the viewport at all.
        // The `* 0.4` also meant the two lines overlapped by 60% of a line at
        // every size, which is what made the offsets look tuned.
        //
        // The stack is the thing being centred now, and `centre_line` refuses
        // the whole of it when the viewport cannot hold both lines. Refusing
        // both rather than drawing one is right here: the second line is the
        // instruction for undoing the state the first line reports, so a
        // viewport that shows "paused" without saying how to resume is worse
        // than one that shows neither and leaves the button to say it.
        let stack = line_h + small_h;
        let Some(top) = centre_line(v, stack) else {
            return;
        };
        centred_in(
            f,
            Rect::new(v.x, top, v.w, line_h),
            "Magnifier paused",
            size,
            OVERLAY0,
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            Rect::new(v.x, top + line_h, v.w, small_h),
            "Esc, or the Pause button, to resume",
            small,
            OVERLAY0,
            FontWeightHint::Regular,
        );
    }

    // ── The chrome ─────────────────────────────────────────────────────────

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        let size = l.big.min(l.header.h * 0.8);
        let right_text = if self.paused {
            "paused".to_string()
        } else {
            trim_zoom(self.zoom())
        };
        let small = (size * 0.7).max(8.0);
        let left = l.header.x + l.pad;
        let right = l.header.right() - l.pad;

        // The reading is measured before the title is placed, and the title's
        // column ends where the reading's begins. It used to be the other way
        // about: the title took a flat `header.w * 0.5` and the reading a flat
        // `* 0.45`, two guesses that sum to less than one only because they were
        // chosen to — and a `pad` at each end plus a reading wider than 45% of a
        // narrow header put the two through each other. Neither number knew
        // what the other was drawing.
        //
        // `.min(right - left)` is the bound, and it is the only one needed for
        // the reading's own left edge: `shows` will not let a band under 110
        // wide reach here and `pad`'s clamp tops out at 10, so the column is at
        // least 90 wide, and a reading cut to the column starts at
        // `right - reading_w`, which is at or after `left` by construction. A
        // `.max(left)` on top of that would be a guard nothing could enter,
        // which is a guard no test could check — so it is a proof here rather
        // than a second clamp. `split` keeps its `.max(left)`, because it
        // subtracts a further `pad` and so genuinely can undershoot.
        let reading_w = text::measure(&right_text, small, FontWeightHint::Bold).min(right - left);
        let split = (right - reading_w - l.pad).max(left);

        // `centre_line` per line, not one `y` for both: the two runs are set at
        // different sizes, so they have different line heights and a single
        // offset centres at most one of them. A header too short for the larger
        // run still shows the reading, which is the half of this band that
        // changes.
        if let Some(y) = centre_line(l.header, text::line_height(size, FontWeightHint::Bold)) {
            push_text(
                f,
                left,
                y,
                "Magnifier",
                size,
                LAVENDER,
                FontWeightHint::Bold,
                split - left,
            );
        }
        if let Some(y) = centre_line(l.header, text::line_height(small, FontWeightHint::Bold)) {
            let x = right - reading_w;
            push_text(
                f,
                x,
                y,
                &right_text,
                small,
                if self.paused { OVERLAY0 } else { GREEN },
                FontWeightHint::Bold,
                right - x,
            );
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.info) {
            return;
        }
        let size = l.font.min(l.info.h * 0.7);
        let line_h = text::line_height(size, FontWeightHint::Regular);
        // The swatch is charged for whether or not there is one, so the text
        // beside it does not change width the moment a colour is picked.
        // `.min(l.info.h)` on top of the `0.6` factor. The factor alone is a
        // bound only while it stays a factor, and the `clamp`'s ceiling of 18
        // is a literal that knows nothing about the band: it is inert today
        // because `shows` will not let a band under 11 tall reach here and
        // 0.6 x 11 is under 18, but the ceiling is the term that would bite
        // first if either number moved, and a square swatch is as tall as it is
        // wide so it is the height that has to be checked.
        let swatch = (l.info.h * 0.6).clamp(0.0, 18.0).min(l.info.h);
        let swatch_rect = Rect::new(
            (l.info.right() - l.pad - swatch).max(l.info.x),
            centre_line(l.info, swatch).unwrap_or(l.info.y),
            swatch,
            swatch,
        );
        if let Some((r, g, b)) = self.picked {
            fill(f, swatch_rect, Color::rgb(r, g, b), swatch * 0.2);
            stroke(f, swatch_rect, SURFACE1, 1.0, swatch * 0.2);
        }

        // The status shares the reading's row rather than stacking under it,
        // which is what `pick_colour_at`'s doc comment has always claimed
        // ("the status line beside it") and what the code did not do. The
        // stacked form was guarded by `line_h + status_h <= info.h`, and that
        // guard is never true: two rows want `2.55 * size` at this font, and
        // `Layout` gives the info band at most `2.28 * size` at *every* window
        // height from 40 to 2160 — the closest it comes is 1.41px short, at
        // h=309. So the status had never once been drawn at any window size.
        // A guard that no input satisfies is not a bound, it is a delete
        // button on the feature behind it, and it reads exactly like a bound;
        // only a mutation that survived pointed at it. The band is a one-line
        // strip by construction (16..=30px against a single font), so the room
        // it actually has is horizontal.
        //
        // A bare factor, where this had a `.max(7.0)` legibility floor. The
        // floor cannot bite: it needs `size` under 7.6, and `size` is
        // `font.min(info.h * 0.7)` with `font` at least 8 and `shows` refusing
        // any band under 11 tall, so `size >= min(8, 7.7) = 7.7` — inert, and
        // inert by a tenth of a pixel, which is a coincidence rather than a
        // margin. It is worth losing rather than keeping, because a floor is
        // the one shape that can make the status *taller* than the reading it
        // sits in, and centring a taller run inside a shorter one's line box
        // lifts it out through the top of the band. Without the floor
        // `status_size <= size` is arithmetic, so `status_h <= line_h` holds by
        // monotonicity and the offset below is non-negative with nothing to
        // prove about reachability. Legibility survives on the same bound that
        // killed the floor: 0.92 x 7.7 is 7.08px at the very worst.
        let status_size = size * 0.92;
        let status_h = text::line_height(status_size, FontWeightHint::Regular);
        let Some(top) = centre_line(l.info, line_h) else {
            return;
        };
        let left = l.info.x + l.pad;
        let run_right = swatch_rect.x - l.pad;

        // Right-aligned against the swatch, so that a status arriving or
        // departing never shifts the reading, and held to its own half of the
        // run so a long message cannot crowd out the standing state it
        // annotates. The separating gap comes out of the *status's* half, not
        // the reading's: charged the other way a maximal status leaves the
        // reading a padding short of half, which is the losing side of a split
        // whose whole point is that the standing state is not the transient
        // one's to spend. `.max(0.0)` twice over because a band narrow enough
        // for the swatch and the paddings to have eaten the run leaves both
        // terms negative.
        let mid = left + ((run_right - left) * 0.5).max(0.0);
        let status_w = text::measure(&self.status, status_size, FontWeightHint::Regular)
            .min((run_right - mid - l.pad).max(0.0));
        let status_x = run_right - status_w;
        // A gap only when there is something to be kept apart from; charging
        // it unconditionally would narrow the reading by a padding for a
        // status that is not there.
        let gap = if status_w > 0.0 { l.pad } else { 0.0 };
        push_text(
            f,
            left,
            top,
            &self.info_line(),
            size,
            SUBTEXT0,
            FontWeightHint::Regular,
            status_x - gap - left,
        );
        push_text(
            f,
            status_x,
            top + (line_h - status_h) / 2.0,
            &self.status,
            status_size,
            TEXT_COLOR,
            FontWeightHint::Regular,
            run_right - status_x,
        );
    }

    /// What a control's button says right now.
    ///
    /// Derived from the state rather than fixed, so a button reports the thing
    /// it toggles instead of merely naming it.
    #[must_use]
    pub fn control_label(&self, target: Target) -> String {
        match target {
            Target::NextMode => self.mode.label().to_string(),
            Target::NextTracking => match self.tracking {
                TrackingMode::FollowMouse => "Mouse".to_string(),
                TrackingMode::FollowFocus => "Focus".to_string(),
                TrackingMode::Manual => "Manual".to_string(),
            },
            Target::NextFilter => self.filter.short().to_string(),
            Target::ToggleCrosshair => if self.crosshair {
                "Cross \u{2713}"
            } else {
                "Cross"
            }
            .to_string(),
            Target::ToggleRuler => match self.ruler {
                Ruler::Off => "Ruler".to_string(),
                Ruler::Measuring { .. } => "Ruler\u{2026}".to_string(),
                Ruler::Done { .. } => "Clear".to_string(),
            },
            Target::TogglePause => if self.paused { "Resume" } else { "Pause" }.to_string(),
            other => CONTROLS
                .iter()
                .find(|(t, _)| *t == other)
                .map_or_else(String::new, |(_, text_str)| (*text_str).to_string()),
        }
    }

    /// Whether a control is showing something switched on.
    fn control_lit(&self, target: Target) -> bool {
        match target {
            Target::ToggleCrosshair => self.crosshair,
            Target::ToggleRuler => self.ruler != Ruler::Off,
            Target::TogglePause => self.paused,
            Target::ToggleHelp => self.show_help,
            Target::NextFilter => self.filter != ColorFilter::None,
            _ => false,
        }
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        if l.controls.w <= 0.0 || l.controls.h <= 0.0 {
            return;
        }
        for (slot, (target, _)) in CONTROLS.iter().enumerate() {
            let r = l.button(l.controls, slot, CONTROLS.len());
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            let lit = self.control_lit(*target);
            button(
                f,
                l,
                r,
                &self.control_label(*target),
                if lit { SURFACE1 } else { SURFACE0 },
                if lit { YELLOW } else { TEXT_COLOR },
            );
            // The hit box goes on in the same pass that drew the button, from
            // the same rectangle, so no arithmetic can put one of them
            // somewhere the other is not.
            f.hit(*target, r);
        }
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        // The backdrop first, then the sheet, then one hit box over the whole
        // window. `hit_test` reads the last box at a point, so while the sheet
        // is up every click anywhere closes it — including one that lands on a
        // button underneath, which would otherwise fire through the sheet.
        fill(f, l.window, Color::rgba(0, 0, 0, 170), 0.0);
        let h = l.help;
        fill(f, h, SURFACE0, 12.0);
        stroke(f, h, SURFACE1, 1.0, 12.0);
        // Every run below is cut to the sheet with `Rect::intersect`, and none
        // of them were before. The reason nothing complained is that this pass
        // scrims the *window*, so it legitimately owns the window and a
        // containment test measuring it against the region it owns has nothing
        // to say about the sheet: a title hanging off the sheet onto the scrim
        // is still inside the window. `centre_line` cannot see it either, since
        // the box a run is centred in is one line tall and a one-line box always
        // fits itself. A bound on a run is only as good as the box it is
        // measured against, and the box with the claim on it is the sheet.
        //
        // The title's box is a nominal `pad` below the sheet's top and one line
        // tall, which hangs off the bottom of any sheet shorter than
        // `pad + title_h`; the footer's is a nominal `foot` above the bottom,
        // which hangs off the top of the same sheet. `intersect` answers `None`
        // for a sheet with no room and cuts all four edges otherwise, so it is
        // both the bound and the refusal.
        // The sheet's inner column, once its own left and right margins are
        // taken out. A sheet narrower than the two margins together leaves a
        // *negative* width here, and every run below would then be pushed at a
        // left edge sitting to the right of its own right edge. Naming the
        // column as a `Rect` is what makes that expressible at all; the
        // alternative -- clamping the column split into `left..=right` -- does
        // not merely fail to bound it, it panics outright, because `f32::clamp`
        // requires `min <= max`, and that panic is how this was found, from a
        // squeezed window in the sheet's own containment test.
        //
        // The early return is a fast path, **not** the bound, and the
        // difference matters to anyone editing below it. An earlier version of
        // this comment called it the bound, which would license loosening the
        // `intersect`s that follow -- and those are what actually hold. With a
        // negative `inner.w` every box built below is a rectangle of negative
        // width, and `Rect::intersect` answers `None` for any of them, so each
        // site already refuses on its own account. Deleting these four lines
        // changes not one command in the frame; the sweep says so, and
        // `mutate.py` records it as an equivalent mutant rather than a hole.
        let inner = Rect::new(h.x + l.pad * 1.5, h.y, h.w - l.pad * 3.0, h.h);
        if inner.is_empty() {
            f.hit(Target::ToggleHelp, l.window);
            return;
        }
        let left = inner.x;
        let right = inner.right();
        let title = (l.big * 0.8).clamp(9.0, 18.0);
        let title_h = text::line_height(title, FontWeightHint::Bold);
        if let Some(box_) = Rect::new(left, h.y + l.pad, inner.w, title_h).intersect(h)
            && let Some(y) = centre_line(box_, title_h)
        {
            push_text(
                f,
                box_.x,
                y,
                HELP_TITLE,
                title,
                BLUE,
                FontWeightHint::Bold,
                box_.right() - box_.x,
            );
        }

        let top = h.y + l.pad * 2.0 + title_h;
        let foot = text::line_height(l.font * 0.8, FontWeightHint::Regular) + l.pad;
        let room = (h.bottom() - foot - top).max(0.0);
        let step = room / HELP_ROWS.len() as f32;
        let size = (step * 0.72).clamp(6.0, l.font);
        // The two columns are split from one number, so neither can be drawn
        // through the other. They were two independent guesses before — the keys
        // took `h.w * 0.36` from `pad * 1.5` and the description started at
        // `h.w * 0.42` — which overlap whenever `pad * 1.5` exceeds six percent
        // of the sheet's width, and `pad` is a floor-bearing `clamp(2, 10)`
        // while the sheet's width is not, so a narrow window is exactly where
        // they cross. Taken as a fraction of `inner.w`, which is positive by
        // the refusal above, the split lands inside `left..=right` by
        // construction and needs no clamp to keep it there.
        let split = left + inner.w * 0.42;
        for (i, (keys, what)) in HELP_ROWS.iter().enumerate() {
            let y = top + i as f32 * step;
            let Some(row) = Rect::new(left, y, inner.w, step).intersect(h) else {
                continue;
            };
            let Some(y) = centre_line(row, text::line_height(size, FontWeightHint::Bold)) else {
                continue;
            };
            push_text(
                f,
                left,
                y,
                keys,
                size,
                YELLOW,
                FontWeightHint::Bold,
                split - left,
            );
            push_text(
                f,
                split,
                y,
                what,
                size,
                TEXT_COLOR,
                FontWeightHint::Regular,
                right - split,
            );
        }

        let small = (l.font * 0.8).max(7.0);
        let small_h = text::line_height(small, FontWeightHint::Regular);
        if let Some(box_) = Rect::new(left, h.bottom() - foot, inner.w, small_h).intersect(h)
            && let Some(y) = centre_line(box_, small_h)
        {
            push_text(
                f,
                box_.x,
                y,
                "Any click, or Esc, closes this",
                small,
                OVERLAY0,
                FontWeightHint::Regular,
                box_.right() - box_.x,
            );
        }
        f.hit(Target::ToggleHelp, l.window);
    }
}

// ── Drawing helpers ────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn stroke(f: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

fn line(f: &mut Frame, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
    if width <= 0.0 {
        return;
    }
    f.push(RenderCommand::Line {
        x1,
        y1,
        x2,
        y2,
        color,
        width,
    });
}

/// Where a run of `height` sits so as to be centred in `band`, or `None` if the
/// band cannot hold it.
///
/// The whole of this app's share of the centring campaign is this function
/// existing and every vertical centring going through it. `band.y + (band.h -
/// height) / 2.0` is not a bound: it is an *offset*, and when the run is taller
/// than the band the offset is negative and the run is placed above the band it
/// was meant to be centred in — by half the overhang at each end, so a centring
/// spills symmetrically out of both sides rather than out of one. Refusing is
/// the only correct answer, because there is nowhere in a band shorter than a
/// line that a line can go.
#[must_use]
fn centre_line(band: Rect, height: f32) -> Option<f32> {
    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)
}

/// Push a run of text starting at `x`, stopped at `limit` pixels further right.
///
/// `limit` is a width and not an `Option<f32>` on purpose. Every caller has a
/// right-hand edge it must not cross, so "no limit" is never the right answer,
/// and an `Option` makes forgetting it look deliberate. The `limit <= 0.0`
/// refusal matters: a caller that computes `right - x` from a column that has
/// been squeezed to nothing hands over a limit of zero or less, and a run
/// ellipsised into a box with no room is a run drawn outside it.
fn push_text(
    f: &mut Frame,
    x: f32,
    y: f32,
    text_str: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    limit: f32,
) {
    if size <= 0.0 || text_str.is_empty() || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: text_str.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A string centred in `r`, horizontally and vertically.
fn centred_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() || size <= 0.0 {
        return;
    }
    // `.min(r.w)`, and a limit measured from `x` rather than from `r.x`. Both
    // halves of one fault: a centred run is inset by half the slack, so a run
    // handed the box's *full* width as its limit may end half the slack past
    // the box's right edge — and the wider the string, the further past. The
    // limit a centred run needs is the distance from where it starts to where
    // the box ends, and the width it is centred by must already have been cut
    // to the box or the inset itself goes negative.
    let w = text::measure(s, size, weight).min(r.w);
    let line_h = text::line_height(size, weight);
    let Some(y) = centre_line(r, line_h) else {
        return;
    };
    let x = r.x + (r.w - w) / 2.0;
    push_text(f, x, y, s, size, color, weight, r.right() - x);
}

/// A filled, labelled control.
fn button(f: &mut Frame, l: &Layout, r: Rect, text_str: &str, back: Color, fore: Color) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(f, r, back, (r.h * 0.25).min(8.0));
    let size = (r.h * 0.42).clamp(6.0, l.font);
    centred_in(f, r, text_str, size, fore, FontWeightHint::Bold);
}

// ── Window ─────────────────────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a key does in
/// a test is what it does on a screen.
pub fn handle_event(app: &mut Magnifier, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Magnifier {
    fn title(&self) -> String {
        "Magnifier".to_string()
    }

    fn app_id(&self) -> String {
        "magnifier".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the clock runs exactly while the view has
    /// somewhere to ease to and stops the moment it arrives.
    ///
    /// An app that leaves this at the default receives no ticks at all — which
    /// is what this one did, so the smooth tracking its `smooth_edges` field
    /// promised could never have eased anything even had anything read it. See
    /// `known-issues.md` lesson 47.
    fn tick_interval(&self) -> Option<Duration> {
        self.easing().then_some(TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against — that is the whole reason it is stored.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Magnifier {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut magnifier = Magnifier::new();
    app::launch("magnifier", &mut magnifier)
}

#[cfg(test)]
// `float_cmp` is allowed here and only here. The exact comparisons below are
// against values copied straight out of a constant — a zoom taken from
// `ZOOM_PRESETS`, a pan step that *is* `PAN_STEP_FAST` — where an approximate
// test would pass on a number the program could never produce. Anywhere a
// result is arrived at by arithmetic these tests use `about` instead.
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    // ── Harness ────────────────────────────────────────────────────────────

    const SIZE: (f32, f32) = Magnifier::SIZE;

    fn app() -> Magnifier {
        Magnifier::new()
    }

    fn down(a: &mut Magnifier, key: Key) -> EventResult {
        probe::key(a, &probe::press(key))
    }

    /// The same key coming back up.
    ///
    /// There is no probe helper for a release, on purpose: `pressed: false` is
    /// the only difference between the two, and spelling it out here keeps the
    /// tests that care about it saying so.
    fn up(a: &mut Magnifier, key: Key) -> EventResult {
        probe::key(
            a,
            &KeyEvent {
                pressed: false,
                ..probe::press(key)
            },
        )
    }

    fn ctrl_down(a: &mut Magnifier, key: Key) -> EventResult {
        probe::key(a, &probe::ctrl(key))
    }

    fn shift_down(a: &mut Magnifier, key: Key) -> EventResult {
        probe::key(a, &probe::shift(key))
    }

    fn click(a: &mut Magnifier, target: Target) -> EventResult {
        probe::click(a, target)
    }

    fn click_at(a: &mut Magnifier, x: f32, y: f32) -> EventResult {
        a.click_at(x, y, MouseButton::Left, SIZE)
    }

    /// A left press at a window point, against whatever size was last drawn.
    ///
    /// Unlike [`click_at`], which resizes to [`SIZE`] first: a test about
    /// *which* size a click is read against cannot begin by setting it.
    fn press_at(a: &mut Magnifier, x: f32, y: f32) -> EventResult {
        handle_event(
            a,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    fn move_to(a: &mut Magnifier, x: f32, y: f32) -> EventResult {
        a.resize(SIZE.0, SIZE.1);
        handle_event(
            a,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            }),
        )
    }

    fn tick(a: &mut Magnifier, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    /// Run the clock until the view has arrived, or say how far it got.
    ///
    /// Bounded, and it says so when the bound is reached. An unbounded `while
    /// a.easing()` reads more naturally and is a trap: an easing step that
    /// stopped closing the gap would spin here forever, and a hung suite says
    /// only "something is wrong somewhere" where a failed assertion names the
    /// fault. This is the same lesson the maze's `pad_to` learned the hard way.
    fn settle(a: &mut Magnifier) {
        for _ in 0..400 {
            if !a.easing() {
                return;
            }
            tick(a, 33);
        }
        panic!(
            "the view never arrived: still at {:?} heading for {:?}",
            a.centre(),
            a.target()
        );
    }

    /// Press `M` until the mode is the one asked for.
    ///
    /// By name rather than by a count of presses, so a test about the docked
    /// panes says what it needs and does not quietly start testing a different
    /// mode the day [`MODES`] is reordered. Bounded, for the same reason
    /// [`settle`] is.
    fn to_mode(a: &mut Magnifier, mode: MagnifyMode) {
        for _ in 0..MODES.len() {
            if a.mode() == mode {
                return;
            }
            down(a, Key::M);
        }
        assert_eq!(a.mode(), mode, "the mode key never reached {mode:?}");
    }

    fn to_tracking(a: &mut Magnifier, tracking: TrackingMode) {
        for _ in 0..TRACKINGS.len() {
            if a.tracking() == tracking {
                return;
            }
            down(a, Key::T);
        }
        assert_eq!(a.tracking(), tracking, "the tracking key never reached it");
    }

    fn to_filter(a: &mut Magnifier, filter: ColorFilter) {
        for _ in 0..FILTERS.len() {
            if a.filter() == filter {
                return;
            }
            down(a, Key::F);
        }
        assert_eq!(
            a.filter(),
            filter,
            "the filter key never reached {filter:?}"
        );
    }

    fn about(a: f32, b: f32, slack: f32) -> bool {
        (a - b).abs() <= slack
    }

    /// Every `Line` command in a frame, as endpoints.
    fn lines(f: &Frame) -> Vec<(f32, f32, f32, f32)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line { x1, y1, x2, y2, .. } => Some((*x1, *y1, *x2, *y2)),
                _ => None,
            })
            .collect()
    }

    /// Every `FillRect` in a frame, as a rectangle and a colour.
    fn fills(f: &Frame) -> Vec<(f32, f32, f32, f32, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((*x, *y, *width, *height, *color)),
                _ => None,
            })
            .collect()
    }

    /// The picture's blocks: the fills inside the viewport that are not the
    /// viewport's own backing rectangle.
    ///
    /// The backing rectangle is a fill inside the viewport too, and it is
    /// neither sampled from the screen nor filtered — so a test that counts
    /// every fill in the viewport finds one of them whatever the picture is or
    /// is not doing.
    fn blocks(f: &Frame, v: Rect) -> Vec<(f32, f32, Color)> {
        fills(f)
            .into_iter()
            .filter(|&(x, y, w, h, _)| {
                v.contains(x + 1.0, y + 1.0) && w < v.w - 1.0 && h < v.h - 1.0
            })
            .map(|(x, y, _, _, c)| (x, y, c))
            .collect()
    }

    /// Every `Text` command in a frame.
    fn texts(f: &Frame) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    // ── Screen-to-window geometry ──────────────────────────────────────────

    #[test]
    fn the_magnified_region_shrinks_as_the_zoom_grows() {
        let pane = Rect::new(0.0, 0.0, 600.0, 400.0);
        let screen = (1920.0, 1080.0);
        let two = source_rect(pane, 2.0, (960.0, 540.0), screen);
        let ten = source_rect(pane, 10.0, (960.0, 540.0), screen);
        assert_eq!((two.w, two.h), (300.0, 200.0), "600x400 at 2x");
        assert_eq!((ten.w, ten.h), (60.0, 40.0), "600x400 at 10x");
        assert_eq!(two.centre(), (960.0, 540.0), "centred on where it is aimed");
        assert_eq!(ten.centre(), (960.0, 540.0), "and still is at 10x");
    }

    #[test]
    fn the_magnified_region_slides_to_stay_on_the_screen() {
        let pane = Rect::new(0.0, 0.0, 600.0, 400.0);
        let screen = (1920.0, 1080.0);
        // Aimed at the very corner. Centring the region on that point would put
        // three quarters of it off the screen, where `sample_pixel` is black —
        // which is what the old `move_center`, clamping the centre rather than
        // the region, produced.
        let corner = source_rect(pane, 2.0, (0.0, 0.0), screen);
        assert_eq!((corner.x, corner.y), (0.0, 0.0), "slid onto the screen");
        assert_eq!(
            (corner.w, corner.h),
            (300.0, 200.0),
            "and not shrunk to fit"
        );

        let far = source_rect(pane, 2.0, (1920.0, 1080.0), screen);
        assert_eq!(
            (far.right(), far.bottom()),
            (1920.0, 1080.0),
            "and slid the other way at the far corner"
        );
    }

    #[test]
    fn a_region_wider_than_the_screen_is_centred_on_it() {
        // 1.5x into a pane wider than 1.5 screens: there is more picture than
        // screen, so there is nothing to slide into and the margins should be
        // even rather than all on one side.
        let pane = Rect::new(0.0, 0.0, 600.0, 400.0);
        let screen = (300.0, 200.0);
        let src = source_rect(pane, 1.0, (0.0, 0.0), screen);
        assert_eq!((src.w, src.h), (600.0, 400.0), "the pane's worth of screen");
        assert_eq!(src.centre(), (150.0, 100.0), "centred on the screen itself");
    }

    #[test]
    fn a_window_point_and_a_screen_point_are_inverses() {
        let pane = Rect::new(40.0, 60.0, 600.0, 400.0);
        let screen = (1920.0, 1080.0);
        let src = source_rect(pane, 4.0, (500.0, 300.0), screen);
        for (x, y) in [(40.0, 60.0), (340.0, 260.0), (639.0, 459.0)] {
            let (sx, sy) = screen_point(pane, src, x, y);
            let (bx, by) = window_point(pane, src, sx, sy);
            assert!(
                about(bx, x, 0.01) && about(by, y, 0.01),
                "{x},{y} -> {sx},{sy} -> {bx},{by}"
            );
        }
        // The middle of the pane is the point the view is aimed at.
        let (cx, cy) = screen_point(pane, src, pane.centre().0, pane.centre().1);
        assert!(
            about(cx, 500.0, 0.01) && about(cy, 300.0, 0.01),
            "middle is {cx},{cy}"
        );
    }

    #[test]
    fn the_sample_grid_is_one_block_a_pixel_until_that_is_too_many() {
        assert_eq!(block_count(1.0), 1, "one screen pixel, one block");
        assert_eq!(block_count(30.0), 30, "thirty pixels, thirty blocks");
        assert_eq!(
            block_count(30.2),
            31,
            "a part-covered pixel still gets a block"
        );
        assert_eq!(
            block_count(0.0),
            1,
            "never nothing, which would divide by zero"
        );
        assert_eq!(
            block_count(4000.0),
            MAX_BLOCKS,
            "a region too wide to draw a block a pixel coarsens instead"
        );
    }

    #[test]
    fn a_pixel_off_the_screen_is_black_and_not_wrapped_around() {
        assert_eq!(sample_pixel(-1, 10, 100, 100), (0, 0, 0), "off the left");
        assert_eq!(sample_pixel(10, -1, 100, 100), (0, 0, 0), "off the top");
        assert_eq!(sample_pixel(100, 10, 100, 100), (0, 0, 0), "off the right");
        assert_eq!(sample_pixel(10, 100, 100, 100), (0, 0, 0), "off the bottom");
        assert_ne!(
            sample_pixel(99, 99, 100, 100),
            sample_pixel(0, 0, 100, 100),
            "and the last pixel inside is not the first one"
        );
    }

    // ── Colour filters ─────────────────────────────────────────────────────

    #[test]
    fn no_filter_leaves_a_pixel_alone() {
        for (r, g, b) in [(0, 0, 0), (255, 255, 255), (17, 129, 240)] {
            assert_eq!(ColorFilter::None.apply(r, g, b), (r, g, b));
        }
    }

    #[test]
    fn inverting_twice_gives_the_pixel_back() {
        for (r, g, b) in [(0u8, 0u8, 0u8), (255, 255, 255), (100, 150, 200)] {
            let once = ColorFilter::Inverted.apply(r, g, b);
            assert_eq!(
                ColorFilter::Inverted.apply(once.0, once.1, once.2),
                (r, g, b)
            );
        }
        assert_eq!(ColorFilter::Inverted.apply(100, 150, 200), (155, 105, 55));
    }

    #[test]
    fn a_high_contrast_filter_splits_at_the_middle_and_keeps_only_two_colours() {
        // Greys either side of the split, chosen so the luma lands on either
        // side of it and nowhere near, so the test is about the rule and not
        // about a rounding.
        let dark = ColorFilter::YellowOnBlack.apply(100, 100, 100);
        let light = ColorFilter::YellowOnBlack.apply(200, 200, 200);
        assert_eq!(dark, (0, 0, 0), "below the split is black");
        assert_eq!(light, (255, 255, 0), "above it is the filter's colour");
        assert_eq!(
            ColorFilter::WhiteOnBlack.apply(200, 200, 200),
            (255, 255, 255)
        );
        assert_eq!(ColorFilter::GreenOnBlack.apply(200, 200, 200), (0, 255, 0));
        // Exactly at the split counts as dark: the test is `> CONTRAST_SPLIT`.
        let at = ColorFilter::luma(CONTRAST_SPLIT, CONTRAST_SPLIT, CONTRAST_SPLIT);
        assert_eq!(at, CONTRAST_SPLIT, "a flat grey's luma is that grey");
        assert_eq!(
            ColorFilter::YellowOnBlack.apply(CONTRAST_SPLIT, CONTRAST_SPLIT, CONTRAST_SPLIT),
            (0, 0, 0),
            "the split itself is on the dark side"
        );
    }

    #[test]
    fn greyscale_puts_the_luma_on_all_three_channels() {
        let (r, g, b) = ColorFilter::Greyscale.apply(255, 0, 0);
        assert_eq!((r, g), (g, b), "one value on all three channels");
        assert_eq!(r, ColorFilter::luma(255, 0, 0), "and it is the luma");
        assert!((70..80).contains(&r), "red's luma is about 76, not {r}");
    }

    #[test]
    fn luma_runs_the_whole_range_because_its_weights_sum_to_one() {
        assert_eq!(ColorFilter::luma(0, 0, 0), 0, "black is nothing");
        assert_eq!(ColorFilter::luma(255, 255, 255), 255, "white is the top");
        assert!(
            ColorFilter::luma(0, 255, 0) > ColorFilter::luma(255, 0, 0),
            "green weighs more than red"
        );
        assert!(
            ColorFilter::luma(255, 0, 0) > ColorFilter::luma(0, 0, 255),
            "and red more than blue"
        );
    }

    #[test]
    fn the_colour_blindness_filters_leave_white_and_black_alone() {
        // Each matrix's rows sum to one, which is what makes it a simulation of
        // a missing cone rather than a tint: greys stay grey and only hues
        // move. A row that did not sum to one would show up here as a white
        // that came back grey.
        for filter in [
            ColorFilter::Protanopia,
            ColorFilter::Deuteranopia,
            ColorFilter::Tritanopia,
        ] {
            assert_eq!(
                filter.apply(255, 255, 255),
                (255, 255, 255),
                "{filter:?} white"
            );
            assert_eq!(filter.apply(0, 0, 0), (0, 0, 0), "{filter:?} black");
            let (r, g, b) = filter.apply(255, 0, 0);
            assert_ne!((r, g, b), (255, 0, 0), "{filter:?} does change pure red");
        }
    }

    #[test]
    fn a_colour_matrix_is_clamped_at_both_ends_not_just_the_top() {
        // `min(255.0)` alone leaves a negative coefficient free to make a
        // negative float, and `as u8` on one saturates to zero by accident
        // rather than by intent. Every matrix that ships has rows summing to
        // exactly one, so neither end of the clamp can be reached through
        // `apply` — which is why `mix` is reachable from here.
        let over = ColorFilter::mix(
            200,
            200,
            200,
            [2.0, 2.0, 2.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        );
        assert_eq!(over.0, 255, "a row that overshoots saturates at white");
        assert_eq!(over.1, 0, "a row of zeroes is black");
        assert_eq!(over.2, 200, "and an identity row is the channel itself");
        let under = ColorFilter::mix(
            10,
            0,
            0,
            [-4.0, 0.0, 0.0],
            [-0.1, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert_eq!(under, (0, 0, 0), "a negative row stops at black");
    }

    #[test]
    fn every_filter_maps_the_corners_of_the_colour_cube_somewhere_sensible() {
        for filter in FILTERS {
            for r in [0u8, 128, 255] {
                for g in [0u8, 128, 255] {
                    for b in [0u8, 128, 255] {
                        let out = filter.apply(r, g, b);
                        // Black in must stay dark and white in must stay light
                        // under every filter here: none of them is a hue
                        // rotation, and one that inverted only some channels
                        // would show up as a mid-grey answer to pure black.
                        if (r, g, b) == (0, 0, 0) && filter != ColorFilter::Inverted {
                            assert_eq!(out, (0, 0, 0), "{filter:?} on black");
                        }
                        if (r, g, b) == (255, 255, 255) && filter != ColorFilter::Inverted {
                            let l = ColorFilter::luma(out.0, out.1, out.2);
                            assert!(l > CONTRAST_SPLIT, "{filter:?} on white came back {out:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_simulation_filter_leaves_a_grey_exactly_as_grey_as_it_was() {
        // The property that pins the matrices, and the one the corners test
        // cannot see: each row must sum to one, which is to say a colour with
        // no hue must come back with no hue and no change in brightness. A
        // dichromacy simulation that darkened or brightened the whole screen
        // would be simulating something nobody has.
        //
        // Black and white alone do not reach it. Black is the zero vector, so
        // it survives any matrix at all; white saturates, so a row summing to
        // 1.4 still comes back 255. Only the greys in between can tell.
        for filter in [
            ColorFilter::None,
            ColorFilter::Greyscale,
            ColorFilter::Protanopia,
            ColorFilter::Deuteranopia,
            ColorFilter::Tritanopia,
        ] {
            for v in [1u8, 17, 64, 128, 200, 254] {
                let out = filter.apply(v, v, v);
                assert_eq!(
                    out,
                    (v, v, v),
                    "{filter:?} turned the grey {v} into {out:?}"
                );
            }
        }
    }

    // ── Zoom ───────────────────────────────────────────────────────────────

    #[test]
    fn the_zoom_walks_the_presets_and_stops_at_both_ends() {
        let mut a = app();
        assert_eq!(a.preset(), DEFAULT_PRESET, "starts at the default preset");
        for _ in 0..40 {
            ctrl_down(&mut a, Key::Equals);
        }
        assert_eq!(
            a.preset(),
            ZOOM_PRESETS.len() - 1,
            "zooming in far past the top stops at the top"
        );
        assert_eq!(a.zoom(), 20.0, "which is 20x");
        for _ in 0..40 {
            ctrl_down(&mut a, Key::Minus);
        }
        assert_eq!(a.preset(), 0, "and out past the bottom stops at the bottom");
        assert_eq!(a.zoom(), 1.5, "which is 1.5x");
    }

    #[test]
    fn the_zoom_is_a_preset_index_so_it_cannot_land_between_two_of_them() {
        let mut a = app();
        for _ in 0..3 {
            ctrl_down(&mut a, Key::Equals);
        }
        assert!(
            ZOOM_PRESETS.contains(&a.zoom()),
            "the zoom {} is not one of the presets",
            a.zoom()
        );
    }

    #[test]
    fn every_preset_has_a_key_of_its_own() {
        // The old program bound six of the ten; 6x, 15x and 20x could only be
        // reached by stepping to them.
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
            Key::Num0,
        ];
        assert_eq!(keys.len(), ZOOM_PRESETS.len(), "a key each");
        let mut reached = Vec::new();
        for key in keys {
            let mut a = app();
            assert_eq!(
                down(&mut a, key),
                EventResult::Consumed,
                "{key:?} does something"
            );
            reached.push(a.zoom());
        }
        assert_eq!(
            reached,
            ZOOM_PRESETS.to_vec(),
            "the number keys reach the presets in order"
        );
    }

    #[test]
    fn ctrl_zero_returns_to_the_default_zoom() {
        let mut a = app();
        down(&mut a, Key::Num9);
        assert_eq!(a.zoom(), 15.0, "moved off the default first");
        ctrl_down(&mut a, Key::Num0);
        assert_eq!(a.preset(), DEFAULT_PRESET, "ctrl-0 is home");
        assert_eq!(a.zoom(), ZOOM_PRESETS[DEFAULT_PRESET]);
    }

    #[test]
    fn a_zoom_button_zooms_the_way_it_is_named() {
        let mut a = app();
        let start = a.preset();
        click(&mut a, Target::ZoomIn);
        assert_eq!(a.preset(), start + 1, "the + button zooms in");
        click(&mut a, Target::ZoomOut);
        assert_eq!(a.preset(), start, "and the - button undoes it");
    }

    #[test]
    fn zooming_in_shows_less_of_the_screen_in_the_same_pane() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let before = a.magnified_source();
        ctrl_down(&mut a, Key::Equals);
        let after = a.magnified_source();
        assert!(
            after.w < before.w && after.h < before.h,
            "a higher zoom must show a smaller region: {before:?} then {after:?}"
        );
    }

    // ── Keys in general ────────────────────────────────────────────────────

    #[test]
    fn a_key_coming_back_up_is_not_a_second_press() {
        // Every key the program answers, not a sample: the old handler had no
        // `pressed` test at all, so each of these fired twice per keystroke.
        let keys = [
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::M,
            Key::T,
            Key::F,
            Key::X,
            Key::L,
            Key::G,
            Key::C,
            Key::R,
            Key::LeftBracket,
            Key::RightBracket,
            Key::Tab,
            Key::Escape,
            Key::H,
            Key::F1,
            Key::Num1,
            Key::Num0,
        ];
        for key in keys {
            let mut a = app();
            assert!(
                a.key_action(&probe::press(key)).is_some(),
                "{key:?} should mean something when pressed"
            );
            assert_eq!(
                up(&mut a, key),
                EventResult::Ignored,
                "{key:?} coming back up must do nothing"
            );
        }
    }

    #[test]
    fn a_modifier_the_program_does_not_use_is_left_alone() {
        let mut a = app();
        for modifiers in [
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let before = a.preset();
            let r = probe::key(&mut a, &probe::press_with(Key::Num9, modifiers));
            assert_eq!(
                r,
                EventResult::Ignored,
                "{modifiers:?} belongs to someone else"
            );
            assert_eq!(a.preset(), before, "and changed nothing");
        }
    }

    #[test]
    fn a_key_the_program_has_no_use_for_is_ignored() {
        let mut a = app();
        assert_eq!(down(&mut a, Key::Q), EventResult::Ignored);
        assert_eq!(down(&mut a, Key::Backspace), EventResult::Ignored);
    }

    // ── Panning ────────────────────────────────────────────────────────────

    #[test]
    fn an_arrow_key_moves_the_view_one_step_in_that_direction() {
        for (key, dx, dy) in [
            (Key::Left, -PAN_STEP, 0.0),
            (Key::Right, PAN_STEP, 0.0),
            (Key::Up, 0.0, -PAN_STEP),
            (Key::Down, 0.0, PAN_STEP),
        ] {
            let mut a = app();
            let (x, y) = a.target();
            down(&mut a, key);
            assert_eq!(a.target(), (x + dx, y + dy), "{key:?} pans by one step");
        }
    }

    #[test]
    fn ctrl_and_an_arrow_key_moves_further_than_the_arrow_alone() {
        let mut a = app();
        let (x, _) = a.target();
        ctrl_down(&mut a, Key::Right);
        let fast = a.target().0 - x;
        assert_eq!(fast, PAN_STEP_FAST, "ctrl-Right is the fast step");
        assert!(fast > PAN_STEP, "and it is the larger of the two");
    }

    #[test]
    fn holding_an_arrow_key_pans_from_where_it_is_heading_not_from_where_it_is() {
        // Panning from the centre would throw away the part of the last pan that
        // had not eased in yet, so a held key would crawl.
        let mut a = app();
        let (x, y) = a.target();
        for _ in 0..5 {
            down(&mut a, Key::Right);
        }
        assert_eq!(
            a.target(),
            (x + PAN_STEP * 5.0, y),
            "five presses are five steps"
        );
        assert!(a.easing(), "and the view is still on its way");
    }

    #[test]
    fn the_view_cannot_be_panned_off_the_screen() {
        let mut a = app();
        for _ in 0..1000 {
            ctrl_down(&mut a, Key::Left);
            ctrl_down(&mut a, Key::Up);
        }
        assert_eq!(a.target(), (0.0, 0.0), "stops at the top-left corner");
        for _ in 0..1000 {
            ctrl_down(&mut a, Key::Right);
            ctrl_down(&mut a, Key::Down);
        }
        assert_eq!(a.target(), a.screen(), "and at the bottom-right one");
    }

    // ── The lens and the docked strip ──────────────────────────────────────

    #[test]
    fn shift_and_an_arrow_key_resizes_the_lens() {
        // The fault this test exists for: the old handler's four `"Left" if
        // shift` arms sat *below* the four `"Left" if !ctrl` ones. Shift-Left
        // has ctrl false, so the earlier arm swallowed it and the lens could not
        // be resized by keyboard at all — and `#![allow(dead_code)]` meant not
        // even a warning said so.
        for (key, dw, dh) in [
            (Key::Left, -LENS_STEP, 0.0),
            (Key::Right, LENS_STEP, 0.0),
            (Key::Up, 0.0, -LENS_STEP),
            (Key::Down, 0.0, LENS_STEP),
        ] {
            let mut a = app();
            let (w, h) = a.lens_size();
            let aim = a.target();
            assert_eq!(
                shift_down(&mut a, key),
                EventResult::Consumed,
                "shift-{key:?} must reach the lens"
            );
            assert_eq!(
                a.lens_size(),
                (w + dw, h + dh),
                "shift-{key:?} resizes the lens"
            );
            assert_eq!(a.target(), aim, "and does not also pan");
        }
    }

    #[test]
    fn the_lens_cannot_be_shrunk_away_or_grown_without_end() {
        let mut a = app();
        for _ in 0..200 {
            shift_down(&mut a, Key::Left);
            shift_down(&mut a, Key::Up);
        }
        assert_eq!(a.lens_size(), (LENS_MIN, LENS_MIN), "stops at the smallest");
        for _ in 0..200 {
            shift_down(&mut a, Key::Right);
            shift_down(&mut a, Key::Down);
        }
        assert_eq!(a.lens_size(), (LENS_MAX, LENS_MAX), "and at the largest");
    }

    #[test]
    fn the_brackets_resize_the_docked_strip_within_bounds() {
        let mut a = app();
        let start = a.dock();
        down(&mut a, Key::RightBracket);
        assert!(a.dock() > start, "] makes the strip taller");
        down(&mut a, Key::LeftBracket);
        assert!(about(a.dock(), start, 0.001), "[ puts it back");
        for _ in 0..100 {
            down(&mut a, Key::LeftBracket);
        }
        assert!(
            about(a.dock(), DOCK_MIN, 0.001),
            "stops at the smallest share"
        );
        for _ in 0..100 {
            down(&mut a, Key::RightBracket);
        }
        assert!(about(a.dock(), DOCK_MAX, 0.001), "and at the largest");
    }

    #[test]
    fn the_docked_strip_takes_the_share_it_says_and_the_rest_is_life_size() {
        let mut a = app();
        to_mode(&mut a, MagnifyMode::DockedTop);
        a.resize(SIZE.0, SIZE.1);
        let l = a.layout();
        let (mag, life) = a.panes();
        assert!(
            about(mag.h, l.viewport.h * a.dock(), 0.5),
            "the strip is {} of a {} viewport, not {}",
            a.dock(),
            l.viewport.h,
            mag.h
        );
        assert!(
            about(mag.h + life.h, l.viewport.h, 0.5),
            "and the two together are the whole viewport"
        );
        assert!(
            mag.y < life.y,
            "docked at the top means the strip is on top"
        );
    }

    #[test]
    fn docking_at_the_bottom_puts_the_strip_at_the_bottom() {
        let mut a = app();
        to_mode(&mut a, MagnifyMode::DockedBottom);
        a.resize(SIZE.0, SIZE.1);
        let (mag, life) = a.panes();
        assert!(mag.y > life.y, "the strip is now below the life-size pane");
        assert!(
            about(mag.bottom(), a.layout().viewport.bottom(), 0.5),
            "and reaches the foot of the viewport"
        );
    }

    #[test]
    fn the_two_panes_never_overlap_in_any_mode_or_at_any_dock_share() {
        for mode in MODES {
            for dock in [DOCK_MIN, 0.33, DOCK_MAX] {
                let l = Layout::new(SIZE.0, SIZE.1, true);
                let (mag, life) = l.panes(mode, dock);
                let overlap = mag.intersect(life);
                assert!(
                    overlap.is_none(),
                    "{mode:?} at {dock}: {mag:?} and {life:?} overlap in {overlap:?}"
                );
                // And both are cut from the viewport, not from the window. A
                // pane that ignored the viewport would still not overlap its
                // partner — it would merely be drawn over the toolbar.
                for (which, pane) in [("magnified", mag), ("life-size", life)] {
                    if pane.w <= 0.0 || pane.h <= 0.0 {
                        continue;
                    }
                    let v = l.viewport;
                    assert!(
                        pane.x >= v.x - 0.01
                            && pane.y >= v.y - 0.01
                            && pane.right() <= v.right() + 0.01
                            && pane.bottom() <= v.bottom() + 0.01,
                        "{mode:?} at {dock}: the {which} pane {pane:?} leaves the viewport {v:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_lens_sits_over_the_part_of_the_screen_it_is_showing() {
        // The old `render_lens` positioned the lens at the pointer and filled it
        // from the centre. Those are the same number only while tracking follows
        // the mouse; in manual tracking the lens sat under your hand and showed
        // somewhere else entirely.
        let mut a = app();
        to_mode(&mut a, MagnifyMode::Lens);
        to_tracking(&mut a, TrackingMode::Manual);
        a.resize(SIZE.0, SIZE.1);
        // Move the pointer well away from the centre; nothing should follow it.
        move_to(&mut a, 60.0, 500.0);

        // Read the lens back out of the frame the window actually drew, rather
        // than asking the program where it put it. Recomputing the placement
        // here with the same formula the program uses would pass however wrong
        // that formula is: it would only be checking the program agrees with
        // itself.
        let f = a.draw(SIZE);
        let lens = f
            .rect_of(|t| *t == Target::Magnified)
            .expect("lens mode draws a magnified box");
        assert_eq!(lens, a.lens_rect(), "and reads back where it was drawn");

        // The lens covers a piece of the life-size view, and the question is
        // *which* piece: it must be the piece the lens is magnifying. So read
        // the point through the life-size pane's own mapping — `screen_point`,
        // which is `window_point`'s inverse and not the function the placement
        // is made of — and it must come back the centre of the picture.
        //
        // Not `self.screen_at(centre of the lens)`: that reads the lens rather
        // than the pane under it, and the lens is *defined* to be showing the
        // centre, so it answers "the centre" wherever the lens happens to be.
        // That is the vacuous form of this test, and it passed against a lens
        // pinned to the pointer.
        let (wx, wy) = a.centre();
        let (_, life) = a.panes();
        let life_src = source_rect(life, 1.0, a.centre(), a.screen());
        let (px, py) = a.pointer();
        let at_pointer = screen_point(life, life_src, px, py);
        assert!(
            (at_pointer.0 - wx).abs() > 40.0 || (at_pointer.1 - wy).abs() > 40.0,
            "the pointer is over screen {at_pointer:?}, too near the centre {:?} \
             for this test to be able to tell the two apart",
            (wx, wy)
        );

        let (cx, cy) = lens.centre();
        let under = screen_point(life, life_src, cx, cy);
        assert!(
            about(under.0, wx, 1.0) && about(under.1, wy, 1.0),
            "the lens is centred over screen {under:?} — that is what is behind \
             it — but it is showing {:?}, and the pointer is over {at_pointer:?}",
            (wx, wy)
        );
    }

    #[test]
    fn a_click_inside_the_lens_is_read_against_the_lens_and_not_the_pane_behind_it() {
        // The lens sits on top of the life-size pane, and the two are at
        // different scales. A point inside the lens therefore has two possible
        // readings, and only one of them is what the user can see there.
        let mut a = app();
        to_mode(&mut a, MagnifyMode::Lens);
        // Focus tracking, not manual: manual refuses to follow a click at all,
        // so the aim could not be read back. Not mouse tracking either, since
        // that would re-aim on the way to the click as well as at it.
        to_tracking(&mut a, TrackingMode::FollowFocus);
        a.resize(SIZE.0, SIZE.1);
        let lens = a.lens_rect();
        // A quarter of the way across the lens: far enough from the middle that
        // the two readings differ, and safely inside it.
        let x = lens.x + lens.w * 0.25;
        let y = lens.y + lens.h * 0.5;

        let lens_src = source_rect(lens, a.zoom(), a.centre(), a.screen());
        let want = screen_point(lens, lens_src, x, y);
        let (_, life) = a.panes();
        let life_src = source_rect(life, 1.0, a.centre(), a.screen());
        let other = screen_point(life, life_src, x, y);
        assert!(
            (want.0 - other.0).abs() > 1.0,
            "at {}x the two readings coincide, so this test proves nothing",
            a.zoom()
        );

        // Through the hit boxes, because that is what decides it. The lens is
        // drawn over the life-size pane and both record a box; only the order
        // they are recorded in makes a click here the magnified one.
        assert_eq!(
            a.target_at(x, y),
            Some(Target::Magnified),
            "a point inside the lens belongs to the magnified view"
        );
        assert_eq!(press_at(&mut a, x, y), EventResult::Consumed);
        let got = a.target();
        assert!(
            about(got.0, want.0, 0.5) && about(got.1, want.1, 0.5),
            "a click in the lens aimed at {got:?}; at the magnified scale that \
             point is {want:?}, at life size {other:?}"
        );
    }

    #[test]
    fn the_lens_is_kept_inside_the_viewport() {
        let mut a = Magnifier::with_screen(1920.0, 1080.0);
        to_mode(&mut a, MagnifyMode::Lens);
        a.resize(SIZE.0, SIZE.1);
        // Aim at each corner of the screen in turn; the lens must stay drawn.
        for corner in [(0.0, 0.0), (1920.0, 0.0), (0.0, 1080.0), (1920.0, 1080.0)] {
            a.apply(Action::JumpTo(corner.0, corner.1));
            settle(&mut a);
            let lens = a.lens_rect();
            let v = a.layout().viewport;
            assert!(
                lens.x >= v.x - 0.5
                    && lens.y >= v.y - 0.5
                    && lens.right() <= v.right() + 0.5
                    && lens.bottom() <= v.bottom() + 0.5,
                "aimed at {corner:?} the lens {lens:?} leaves the viewport {v:?}"
            );
        }
    }

    // ── Tracking ───────────────────────────────────────────────────────────

    #[test]
    fn the_tracking_key_walks_the_three_settings_and_comes_back_round() {
        // The order is written out here rather than read from `TRACKINGS`. A
        // test that walks the same list the program walks agrees with the
        // program whatever order the list is in — which is to say it is not a
        // test of the order at all.
        assert_eq!(
            TRACKINGS,
            [
                TrackingMode::FollowMouse,
                TrackingMode::FollowFocus,
                TrackingMode::Manual,
            ],
            "the settings are in this order"
        );
        let mut a = app();
        let mut seen = vec![a.tracking()];
        for _ in 0..3 {
            down(&mut a, Key::T);
            seen.push(a.tracking());
        }
        assert_eq!(
            seen,
            vec![
                TrackingMode::FollowMouse,
                TrackingMode::FollowFocus,
                TrackingMode::Manual,
                TrackingMode::FollowMouse,
            ],
            "and T walks them in it, and comes back round"
        );
    }

    #[test]
    fn following_the_mouse_re_aims_the_view_when_the_pointer_moves() {
        let mut a = app();
        assert_eq!(a.tracking(), TrackingMode::FollowMouse, "the default");
        a.resize(SIZE.0, SIZE.1);
        let before = a.target();
        let (mag, _) = a.panes();
        let r = move_to(&mut a, mag.x + 20.0, mag.y + 20.0);
        assert_eq!(r, EventResult::Consumed, "the motion was used");
        assert_ne!(a.target(), before, "and moved the view");
    }

    #[test]
    fn the_other_two_trackings_leave_the_pointer_as_just_a_pointer() {
        for tracking in [TrackingMode::FollowFocus, TrackingMode::Manual] {
            let mut a = app();
            while a.tracking() != tracking {
                down(&mut a, Key::T);
            }
            a.resize(SIZE.0, SIZE.1);
            let before = a.target();
            let (mag, _) = a.panes();
            let r = move_to(&mut a, mag.x + 20.0, mag.y + 20.0);
            assert_eq!(r, EventResult::Ignored, "{tracking:?} ignores mere motion");
            assert_eq!(a.target(), before, "{tracking:?} did not move the view");
        }
    }

    #[test]
    fn a_click_in_the_picture_aims_at_it_unless_tracking_is_manual() {
        for tracking in [
            TrackingMode::FollowMouse,
            TrackingMode::FollowFocus,
            TrackingMode::Manual,
        ] {
            let mut a = app();
            while a.tracking() != tracking {
                down(&mut a, Key::T);
            }
            a.resize(SIZE.0, SIZE.1);
            let before = a.target();
            let (mag, _) = a.panes();
            click_at(&mut a, mag.x + 15.0, mag.y + 15.0);
            if tracking == TrackingMode::Manual {
                assert_eq!(a.target(), before, "manual tracking means what it says");
            } else {
                assert_ne!(a.target(), before, "{tracking:?} follows a click");
            }
        }
    }

    #[test]
    fn a_click_aims_at_the_screen_point_under_the_pixel_it_landed_on() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let (mag, _) = a.panes();
        let (x, y) = (mag.x + 30.0, mag.y + 40.0);
        // Worked out from the region the pane is *showing*, not by asking
        // `screen_at` — which is the very function the click handler calls, so
        // comparing against it would only prove the program agrees with itself.
        // The pane shows `src`, stretched to fill it; a pixel a fraction of the
        // way across the pane is that same fraction of the way across `src`.
        let src = a.magnified_source();
        let want = (
            src.x + (x - mag.x) / mag.w * src.w,
            src.y + (y - mag.y) / mag.h * src.h,
        );
        click_at(&mut a, x, y);
        assert!(
            about(a.target().0, want.0, 0.01) && about(a.target().1, want.1, 0.01),
            "clicked at {:?} which is screen {want:?}, but aimed at {:?}",
            (x, y),
            a.target()
        );
    }

    #[test]
    fn a_mouse_button_the_program_does_not_use_does_nothing() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let (mag, _) = a.panes();
        let before = a.target();
        for button in [MouseButton::Right, MouseButton::Middle] {
            let r = a.click_at(mag.x + 15.0, mag.y + 15.0, button, SIZE);
            assert_eq!(r, EventResult::Ignored, "{button:?} is not ours");
        }
        assert_eq!(a.target(), before, "and nothing moved");
    }

    // ── Pause ──────────────────────────────────────────────────────────────

    #[test]
    fn pausing_stops_every_control_and_not_merely_the_picture() {
        // The old program read `enabled` only in `render`, so every key still
        // worked while "paused" and you found out what you had done on resume.
        let mut a = app();
        down(&mut a, Key::Escape);
        assert!(a.paused(), "Esc pauses");
        let before = (a.preset(), a.mode(), a.filter(), a.target(), a.tracking());
        ctrl_down(&mut a, Key::Equals);
        down(&mut a, Key::M);
        down(&mut a, Key::F);
        down(&mut a, Key::T);
        down(&mut a, Key::Right);
        assert_eq!(
            (a.preset(), a.mode(), a.filter(), a.target(), a.tracking()),
            before,
            "nothing may change while paused"
        );
    }

    #[test]
    fn pausing_leaves_the_ways_out_of_it_working() {
        let mut a = app();
        down(&mut a, Key::Escape);
        assert!(a.paused());
        down(&mut a, Key::H);
        assert!(a.show_help(), "the shortcuts still open while paused");
        down(&mut a, Key::H);
        down(&mut a, Key::Tab);
        assert!(!a.chrome(), "and the chrome still hides");
        down(&mut a, Key::Escape);
        assert!(!a.paused(), "and Esc still resumes");
    }

    #[test]
    fn the_pause_button_pauses_and_then_says_resume() {
        let mut a = app();
        assert_eq!(a.control_label(Target::TogglePause), "Pause");
        click(&mut a, Target::TogglePause);
        assert!(a.paused());
        assert_eq!(
            a.control_label(Target::TogglePause),
            "Resume",
            "a button that pauses must offer the way back"
        );
        click(&mut a, Target::TogglePause);
        assert!(!a.paused(), "and take it");
    }

    #[test]
    fn a_paused_window_says_so_instead_of_drawing_a_stale_picture() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let v = a.layout().viewport;
        let running = blocks(&a.draw(SIZE), v);
        assert!(
            running.len() > 50,
            "a running window draws a picture made of many blocks, not {}",
            running.len()
        );

        down(&mut a, Key::Escape);
        let f = a.draw(SIZE);
        let said = texts(&f);
        assert!(
            said.iter().any(|t| t == "Magnifier paused"),
            "the window should say, in as many words, that it is paused: {said:?}"
        );
        // And it must actually stop drawing the picture. Saying "paused" over a
        // frozen screenshot is exactly the fault: the user cannot tell a paused
        // magnifier from one whose screen stopped changing.
        let stopped = blocks(&f, v);
        assert!(
            stopped.len() <= 2,
            "a paused window should draw no picture, but drew {} blocks",
            stopped.len()
        );
    }

    // ── Filters through the window ─────────────────────────────────────────

    #[test]
    fn the_filter_key_walks_all_nine_filters_and_comes_back_round() {
        let mut a = app();
        let mut seen = vec![a.filter()];
        for _ in 0..FILTERS.len() {
            down(&mut a, Key::F);
            seen.push(a.filter());
        }
        assert_eq!(seen[0], seen[FILTERS.len()], "the cycle closes");
        assert_eq!(
            &seen[..FILTERS.len()],
            &FILTERS[..],
            "and walks them in the stated order"
        );
    }

    #[test]
    fn the_filter_button_names_the_filter_it_is_showing() {
        let mut a = app();
        for filter in FILTERS {
            to_filter(&mut a, filter);
            assert_eq!(
                a.control_label(Target::NextFilter),
                filter.short(),
                "the button should say which filter is on"
            );
        }
    }

    #[test]
    fn the_filter_reaches_the_pixels_the_window_draws() {
        // Not just the model field: the whole point of a filter is what comes
        // out of the drawing pass.
        // Not just the model field: the whole point of a filter is what comes
        // out of the drawing pass — and `assert_ne!` on two lists of fills is
        // satisfied by *any* difference at all, including one block moving.
        // Inverting is checked here block by block, against the arithmetic.
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let v = a.layout().viewport;
        let plain = blocks(&a.draw(SIZE), v);
        to_filter(&mut a, ColorFilter::Inverted);
        let inverted = blocks(&a.draw(SIZE), v);
        assert!(!plain.is_empty(), "there is a picture to filter");
        assert_eq!(
            plain.len(),
            inverted.len(),
            "a filter changes the colours, not the blocks"
        );
        let mut differed = 0;
        for (&(px, py, pc), &(ix, iy, ic)) in plain.iter().zip(inverted.iter()) {
            assert!(
                about(px, ix, 0.01) && about(py, iy, 0.01),
                "block moved from {:?} to {:?}",
                (px, py),
                (ix, iy)
            );
            assert_eq!(
                (ic.r, ic.g, ic.b),
                (255 - pc.r, 255 - pc.g, 255 - pc.b),
                "the block at {:?} was {pc:?} and inverts to {ic:?}",
                (px, py)
            );
            if pc != ic {
                differed += 1;
            }
        }
        assert!(
            differed > 0,
            "a mid-grey picture would invert to itself; this one must not"
        );
    }

    // ── The ruler ──────────────────────────────────────────────────────────

    #[test]
    fn the_ruler_key_opens_it_closes_it_and_then_clears_it() {
        let mut a = app();
        assert_eq!(a.ruler(), Ruler::Off, "no ruler to start with");
        down(&mut a, Key::R);
        assert!(
            matches!(a.ruler(), Ruler::Measuring { .. }),
            "R once starts a measurement"
        );
        a.apply(Action::JumpTo(1000.0, 600.0));
        settle(&mut a);
        down(&mut a, Key::R);
        assert!(
            matches!(a.ruler(), Ruler::Done { .. }),
            "R again finishes it"
        );
        down(&mut a, Key::R);
        assert_eq!(a.ruler(), Ruler::Off, "and a third clears it");
    }

    #[test]
    fn a_measurement_runs_between_the_two_points_it_was_taken_between() {
        let mut a = app();
        a.apply(Action::JumpTo(100.0, 100.0));
        settle(&mut a);
        down(&mut a, Key::R);
        a.apply(Action::JumpTo(400.0, 500.0));
        settle(&mut a);
        down(&mut a, Key::R);
        // 300 across and 400 down is 500 by Pythagoras — a diagonal, which the
        // old code could not have reported: it took min_x, max_x and min_y and
        // never looked at end_y at all.
        assert!(
            about(a.ruler().screen_length(), 500.0, 0.5),
            "a 300 by 400 measurement is 500 long, not {}",
            a.ruler().screen_length()
        );
    }

    #[test]
    fn the_length_as_shown_is_the_screen_length_multiplied_by_the_zoom() {
        // The old ruler divided by the zoom, so magnifying a thing made the
        // reading of it smaller.
        let done = Ruler::Done {
            start: (0.0, 0.0),
            end: (100.0, 0.0),
        };
        assert!(about(done.screen_length(), 100.0, 0.001));
        assert!(
            about(done.magnified_length(4.0), 400.0, 0.001),
            "100 screen pixels at 4x covers 400 pixels of window"
        );
        assert!(
            done.magnified_length(4.0) > done.screen_length(),
            "a magnified thing must read longer, not shorter"
        );
    }

    #[test]
    fn a_diagonal_measurement_is_drawn_as_a_diagonal() {
        let mut a = app();
        to_tracking(&mut a, TrackingMode::Manual);
        a.apply(Action::JumpTo(900.0, 500.0));
        settle(&mut a);
        down(&mut a, Key::R);
        a.apply(Action::JumpTo(1000.0, 600.0));
        settle(&mut a);
        down(&mut a, Key::R);
        a.resize(SIZE.0, SIZE.1);
        let drawn = lines(&a.draw(SIZE));
        assert!(
            drawn
                .iter()
                .any(|(x1, y1, x2, y2)| (x1 - x2).abs() > 1.0 && (y1 - y2).abs() > 1.0),
            "a measurement taken across and down must be drawn across and down: {drawn:?}"
        );
    }

    #[test]
    fn the_ruler_button_says_which_of_the_three_things_it_will_do() {
        let mut a = app();
        assert_eq!(a.control_label(Target::ToggleRuler), "Ruler");
        click(&mut a, Target::ToggleRuler);
        assert_eq!(a.control_label(Target::ToggleRuler), "Ruler\u{2026}");
        click(&mut a, Target::ToggleRuler);
        assert_eq!(a.control_label(Target::ToggleRuler), "Clear");
        click(&mut a, Target::ToggleRuler);
        assert_eq!(a.control_label(Target::ToggleRuler), "Ruler");
    }

    // ── The colour picker ──────────────────────────────────────────────────

    #[test]
    fn picking_a_colour_reports_one_and_the_readout_says_where_it_came_from() {
        let mut a = app();
        assert_eq!(a.picked(), None, "nothing picked to start with");
        assert!(
            a.info_line().contains("no pick"),
            "and the info line says so"
        );
        down(&mut a, Key::C);
        let picked = a.picked().expect("C picks the colour under the centre");
        assert_eq!(
            picked,
            sample_pixel(
                a.centre().0 as i32,
                a.centre().1 as i32,
                a.screen().0 as i32,
                a.screen().1 as i32
            ),
            "and it is the pixel the view is centred on"
        );
        assert!(
            a.info_line().contains(&hex_of(picked)),
            "the info line should carry the colour: {}",
            a.info_line()
        );
    }

    #[test]
    fn the_readout_gives_the_filtered_colour_and_the_unfiltered_one_beside_it() {
        let mut a = app();
        to_filter(&mut a, ColorFilter::Inverted);
        down(&mut a, Key::C);
        let screen = sample_pixel(
            a.centre().0 as i32,
            a.centre().1 as i32,
            a.screen().0 as i32,
            a.screen().1 as i32,
        );
        let shown = ColorFilter::Inverted.apply(screen.0, screen.1, screen.2);
        assert_eq!(
            a.picked(),
            Some(shown),
            "the swatch shows what the picture shows"
        );
        assert!(
            a.status().contains(&hex_of(screen)),
            "and the status names the screen colour too: {}",
            a.status()
        );
    }

    #[test]
    fn a_picked_colour_puts_a_swatch_in_the_info_band() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let before = fills(&a.draw(SIZE)).len();
        down(&mut a, Key::C);
        let after = fills(&a.draw(SIZE)).len();
        assert!(
            after > before,
            "a picked colour should add a swatch: {before} then {after}"
        );
    }

    #[test]
    fn the_picker_can_be_reached_by_its_button_as_well_as_its_key() {
        let mut a = app();
        click(&mut a, Target::PickColour);
        assert!(a.picked().is_some(), "the Pick button picks");
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_window_is_asked_for_a_clock_exactly_while_the_view_is_moving() {
        // Lesson 47: an app that keeps time but leaves `tick_interval` at the
        // default receives no ticks at all. This one was its seventh case — the
        // `smooth_edges` field it carried could never have eased anything.
        let mut a = app();
        assert_eq!(a.tick_interval(), None, "nothing to do, no clock wanted");
        down(&mut a, Key::Right);
        assert_eq!(a.tick_interval(), Some(TICK), "a pan wants the clock");
        settle(&mut a);
        assert_eq!(a.tick_interval(), None, "and gives it back on arrival");
    }

    #[test]
    fn a_tick_moves_the_view_toward_its_target_and_then_stops_asking() {
        let mut a = app();
        let start = a.centre();
        down(&mut a, Key::Right);
        assert_ne!(a.target(), start, "the target moved at once");
        assert_eq!(a.centre(), start, "the view has not caught up yet");
        assert_eq!(tick(&mut a, 33), EventResult::Consumed, "a tick does work");
        assert_ne!(a.centre(), start, "and moves the view");
        settle(&mut a);
        assert_eq!(a.centre(), a.target(), "which lands exactly on the target");
        assert_eq!(
            tick(&mut a, 33),
            EventResult::Ignored,
            "a tick with nothing to do asks for no repaint"
        );
    }

    #[test]
    fn the_easing_goes_by_the_time_that_passed_not_by_the_interval_asked_for() {
        // The interval is a floor, not a promise. Stepping by `TICK` regardless
        // makes a pan run slow by however much the loop was busy, and silently.
        let mut slow = app();
        let mut fast = app();
        down(&mut slow, Key::Right);
        down(&mut fast, Key::Right);
        tick(&mut slow, 200);
        tick(&mut fast, 20);
        let slow_gap = (slow.target().0 - slow.centre().0).abs();
        let fast_gap = (fast.target().0 - fast.centre().0).abs();
        assert!(
            slow_gap < fast_gap,
            "200 ms should close more of the gap than 20 ms: {slow_gap} vs {fast_gap}"
        );
    }

    #[test]
    fn switching_smoothing_off_makes_the_view_arrive_at_once() {
        let mut a = app();
        assert!(a.smooth(), "smoothing is on by default");
        down(&mut a, Key::G);
        assert!(!a.smooth(), "G turns it off");
        down(&mut a, Key::Right);
        assert_eq!(
            a.centre(),
            a.target(),
            "and the view jumps rather than eases"
        );
        assert_eq!(a.tick_interval(), None, "so no clock is wanted at all");
    }

    #[test]
    fn switching_smoothing_off_mid_pan_does_not_strand_the_view() {
        // Nothing would ask for another tick, so a view left part-way would sit
        // there until something else happened to move it.
        let mut a = app();
        down(&mut a, Key::Right);
        tick(&mut a, 5);
        assert!(a.easing(), "part-way through the pan");
        down(&mut a, Key::G);
        assert_eq!(
            a.centre(),
            a.target(),
            "it arrives rather than stopping short"
        );
        assert!(!a.easing());
    }

    #[test]
    fn a_tick_never_overshoots_however_long_it_says_it_was() {
        let mut a = app();
        let start = a.centre();
        down(&mut a, Key::Right);
        let target = a.target();
        tick(&mut a, 10_000);
        assert!(
            a.centre().0 >= start.0 && a.centre().0 <= target.0,
            "a huge tick landed at {:?}, outside {start:?}..{target:?}",
            a.centre()
        );
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn the_layout_comes_from_the_window_it_is_drawn_in() {
        // The old program's layout was `width: 800.0, height: 600.0`, assigned
        // once in `new` and read by every drawing function — so every rectangle
        // in the window was for a window of a size nothing guaranteed.
        for (w, h) in [(400.0, 300.0), (820.0, 620.0), (1600.0, 1000.0)] {
            let l = Layout::new(w, h, true);
            assert_eq!(l.window, Rect::new(0.0, 0.0, w, h), "{w}x{h}");
            assert!(
                l.viewport.right() <= w + 0.01 && l.viewport.bottom() <= h + 0.01,
                "{w}x{h}: the viewport {:?} leaves the window",
                l.viewport
            );
            assert!(
                l.viewport.w > 0.0 && l.viewport.h > 0.0,
                "{w}x{h}: nothing to see"
            );
        }
    }

    #[test]
    fn the_viewport_keeps_its_share_of_every_window() {
        // Swept rather than sampled at three sizes. The share is what makes the
        // bands drop, and three chosen heights can sit either side of every
        // threshold and never catch a band that overstays by a few pixels.
        for w in [320.0_f32, 820.0, 2000.0] {
            let mut h = 60.0_f32;
            while h <= 1400.0 {
                let l = Layout::new(w, h, true);
                assert!(
                    l.viewport.h >= h * VIEWPORT_SHARE - 1.0,
                    "{w}x{h}: the picture got {} of a promised {}",
                    l.viewport.h,
                    h * VIEWPORT_SHARE
                );
                h += 5.0;
            }
        }
    }

    #[test]
    fn the_bands_are_dropped_whole_in_the_stated_order_when_the_window_shrinks() {
        // Which band goes first, checked by shrinking until each disappears.
        // A shrunk band is a band you cannot read that still costs the picture
        // its height, so they go whole or not at all.
        let mut order = Vec::new();
        let mut present = [true, true, true];
        let mut h = 900.0_f32;
        while h > 40.0 {
            let l = Layout::new(820.0, h, true);
            let now = [
                !l.header.is_empty(),
                !l.info.is_empty(),
                !l.controls.is_empty(),
            ];
            for i in 0..3 {
                if present[i] && !now[i] {
                    order.push(i);
                }
            }
            present = now;
            h -= 5.0;
        }
        // The order is spelled out, not read from `BAND_DROP_ORDER`. Comparing
        // the observed order against the very constant that produced it passes
        // for any order at all.
        assert_eq!(
            order,
            vec![0, 2, 1],
            "the bands should go header, controls, info"
        );
        assert_eq!(
            BAND_DROP_ORDER,
            [0, 2, 1],
            "and the constant should say the same"
        );
    }

    #[test]
    fn a_dropped_band_is_nothing_at_all_rather_than_a_strip_no_pixels_tall() {
        let l = Layout::new(820.0, 120.0, true);
        assert!(
            l.header.is_empty(),
            "the header is gone in a window this short"
        );
        assert_eq!(
            l.header,
            Rect::EMPTY,
            "and gone means empty, not zero-height"
        );
    }

    #[test]
    fn the_viewport_never_sits_on_top_of_a_band_that_is_still_there() {
        for h in [80.0_f32, 120.0, 200.0, 400.0, 620.0, 1200.0] {
            let l = Layout::new(820.0, h, true);
            for (name, band) in [
                ("header", l.header),
                ("info", l.info),
                ("controls", l.controls),
            ] {
                if band.is_empty() {
                    continue;
                }
                assert!(
                    l.viewport.intersect(band).is_none(),
                    "at {h} tall the viewport {:?} covers the {name} {band:?}",
                    l.viewport
                );
            }
        }
    }

    #[test]
    fn hiding_the_chrome_gives_the_whole_window_to_the_picture() {
        let mut a = app();
        a.resize(SIZE.0, SIZE.1);
        let with = a.layout().viewport;
        down(&mut a, Key::Tab);
        assert!(!a.chrome(), "Tab hides the chrome");
        let without = a.layout().viewport;
        assert_eq!(without, Rect::new(0.0, 0.0, SIZE.0, SIZE.1), "all of it");
        assert!(without.h > with.h, "which is more than it had");
        down(&mut a, Key::Tab);
        assert_eq!(a.layout().viewport, with, "and Tab brings the chrome back");
    }

    #[test]
    fn the_picture_is_still_usable_in_a_window_too_small_for_any_chrome() {
        let tiny = (150.0, 90.0);
        let mut a = app();
        a.resize(tiny.0, tiny.1);
        let l = a.layout();
        assert!(
            l.viewport.w > 0.0 && l.viewport.h > 0.0,
            "there is still a picture"
        );
        let (mag, _) = a.panes();
        assert!(mag.w > 0.0 && mag.h > 0.0, "and it is a pane you can click");
        let f = a.draw(tiny);
        assert!(f.is_balanced(), "and the frame is still well formed");
        assert_eq!(
            f.hit_test(mag.centre().0, mag.centre().1),
            Some(Target::Magnified),
            "and a click in it still reaches the picture"
        );
    }

    #[test]
    fn a_button_is_clickable_exactly_where_it_is_drawn() {
        let a = app();
        let f = a.draw(SIZE);
        for (target, _) in CONTROLS {
            let r = f.rect_of(|t| *t == target).unwrap_or_else(|| {
                panic!("{target:?} was drawn without a hit box");
            });
            for (dx, dy, inside) in [
                (r.w / 2.0, r.h / 2.0, true),
                (1.0, 1.0, true),
                (r.w - 1.0, r.h - 1.0, true),
                (-4.0, r.h / 2.0, false),
                (r.w + 4.0, r.h / 2.0, false),
            ] {
                let hit = f.hit_test(r.x + dx, r.y + dy);
                if inside {
                    assert_eq!(hit, Some(target), "{target:?} at {dx},{dy} inside its box");
                } else {
                    assert_ne!(hit, Some(target), "{target:?} at {dx},{dy} outside it");
                }
            }
        }
    }

    #[test]
    fn every_control_the_frame_records_is_wired_to_something() {
        // A hit box that leads nowhere is a button that does nothing, which is
        // exactly what the old toolbar's `[H]elp [M]ode [T]rack [F]ilter` was.
        let a = app();
        let recorded: Vec<Target> = a.draw(SIZE).hits().iter().map(|(t, _)| *t).collect();
        assert!(
            recorded.len() >= CONTROLS.len(),
            "only {} hit boxes for {} buttons and a picture",
            recorded.len(),
            CONTROLS.len()
        );
        for target in recorded {
            let mut fresh = app();
            assert_eq!(
                probe::click(&mut fresh, target),
                EventResult::Consumed,
                "{target:?} has a hit box but nothing happens when it is clicked"
            );
        }
    }

    #[test]
    fn a_click_is_read_against_the_size_the_window_was_last_drawn_at() {
        let small = (360.0, 280.0);
        let mut a = app();
        // Drawn small, so the buttons are where a small window puts them.
        let f = a.draw(small);
        let r = f
            .rect_of(|t| *t == Target::ToggleCrosshair)
            .expect("the crosshair button is drawn even in a small window");
        let before = a.crosshair();
        a.click_at(r.centre().0, r.centre().1, MouseButton::Left, small);
        assert_ne!(a.crosshair(), before, "the click landed on the button");
    }

    #[test]
    fn rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against() {
        let mut a = app();
        let small = (360.0, 280.0);
        let _ = a.render(small.0, small.1);
        assert_eq!(
            a.layout().window,
            Rect::new(0.0, 0.0, small.0, small.1),
            "the model reads back the size it was just drawn at"
        );

        // And the reading-back has consequences: a click is answered against
        // the small window, not against whatever the default was. Both halves
        // are needed — a model that recorded the size and then ignored it when
        // hit-testing would satisfy the assertion above.
        let (mag, _) = a.panes();
        let (x, y) = (mag.x + 4.0, mag.y + 4.0);
        assert_eq!(
            a.target_at(x, y),
            Some(Target::Magnified),
            "{:?} is over the small window's magnified pane",
            (x, y)
        );
        let before = a.target();
        assert_eq!(
            press_at(&mut a, x, y),
            EventResult::Consumed,
            "and a press there lands on it"
        );
        assert_ne!(a.target(), before, "and re-aims the view");
    }

    #[test]
    fn a_resize_event_is_believed() {
        let mut a = app();
        let r = handle_event(
            &mut a,
            &Event::Resize {
                width: 500,
                height: 400,
            },
        );
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(a.layout().window, Rect::new(0.0, 0.0, 500.0, 400.0));
    }

    #[test]
    fn the_frame_is_well_formed_at_every_size_mode_and_state() {
        for size in [
            (120.0, 90.0),
            (360.0, 280.0),
            (820.0, 620.0),
            (1900.0, 1200.0),
        ] {
            for mode in MODES {
                for &(help, paused) in &[(false, false), (true, false), (false, true)] {
                    let mut a = app();
                    to_mode(&mut a, mode);
                    if help {
                        down(&mut a, Key::H);
                    }
                    if paused {
                        down(&mut a, Key::Escape);
                    }
                    let f = a.draw(size);
                    assert!(
                        f.is_balanced(),
                        "{size:?} {mode:?} help={help} paused={paused}: clips left open"
                    );
                }
            }
        }
    }

    // ── The shortcuts sheet ────────────────────────────────────────────────

    #[test]
    fn the_sheet_opens_on_either_of_its_keys_and_closes_on_any_of_the_four() {
        for open in [Key::H, Key::F1] {
            for close in [Key::Escape, Key::H, Key::F1, Key::Enter, Key::Space] {
                let mut a = app();
                down(&mut a, open);
                assert!(a.show_help(), "{open:?} opens the sheet");
                down(&mut a, close);
                assert!(!a.show_help(), "{close:?} should close it");
            }
        }
    }

    #[test]
    fn the_open_sheet_swallows_the_keys_that_are_not_about_it() {
        let mut a = app();
        down(&mut a, Key::H);
        let before = (a.preset(), a.mode(), a.filter(), a.target(), a.paused());
        for key in [
            Key::Num9,
            Key::M,
            Key::F,
            Key::Right,
            Key::T,
            Key::C,
            Key::R,
        ] {
            assert_eq!(
                down(&mut a, key),
                EventResult::Ignored,
                "{key:?} belongs to the sheet's dismissal, not to the program"
            );
        }
        assert_eq!(
            (a.preset(), a.mode(), a.filter(), a.target(), a.paused()),
            before,
            "and nothing behind the sheet moved"
        );
        assert!(a.show_help(), "the sheet is still up");
    }

    #[test]
    fn a_click_anywhere_while_the_sheet_is_up_closes_it_and_reaches_nothing_behind() {
        let mut a = app();
        down(&mut a, Key::H);
        let before = a.crosshair();
        // Aimed straight at the crosshair button, which is underneath the sheet.
        let plain = app();
        let r = plain
            .draw(SIZE)
            .rect_of(|t| *t == Target::ToggleCrosshair)
            .expect("the crosshair button exists");
        click_at(&mut a, r.centre().0, r.centre().1);
        assert!(!a.show_help(), "the click closed the sheet");
        assert_eq!(
            a.crosshair(),
            before,
            "and did not fire the button under it"
        );
    }

    #[test]
    fn while_the_sheet_is_up_every_point_in_the_window_belongs_to_it() {
        let mut a = app();
        down(&mut a, Key::H);
        let f = a.draw(SIZE);
        for (x, y) in [
            (1.0, 1.0),
            (SIZE.0 / 2.0, SIZE.1 / 2.0),
            (SIZE.0 - 1.0, SIZE.1 - 1.0),
            (SIZE.0 - 1.0, 1.0),
        ] {
            assert_eq!(
                f.hit_test(x, y),
                Some(Target::ToggleHelp),
                "the sheet should cover {x},{y}"
            );
        }
    }

    #[test]
    fn the_sheet_lists_a_shortcut_for_each_thing_it_claims_to() {
        let mut a = app();
        down(&mut a, Key::H);
        let said = texts(&a.draw(SIZE));
        assert!(said.iter().any(|t| t == HELP_TITLE), "the sheet is titled");
        for (keys, what) in HELP_ROWS {
            assert!(
                said.iter().any(|t| t == keys),
                "the sheet should list {keys}"
            );
            assert!(said.iter().any(|t| t == what), "and explain it: {what}");
        }
    }

    #[test]
    fn the_help_button_and_the_help_key_are_the_same_switch() {
        let mut a = app();
        click(&mut a, Target::ToggleHelp);
        assert!(a.show_help(), "the button opens the sheet");
        down(&mut a, Key::Escape);
        assert!(!a.show_help(), "and the key closes it");
    }

    // ── The window ─────────────────────────────────────────────────────────

    #[test]
    fn the_window_names_itself() {
        let a = app();
        assert_eq!(a.title(), "Magnifier");
        assert_eq!(a.app_id(), "magnifier");
        let (w, h) = a.initial_size();
        assert!(w > 0 && h > 0, "and asks for a window of some size");
    }

    #[test]
    fn a_close_request_ends_the_program() {
        let mut a = app();
        assert!(
            matches!(a.on_event(&Event::CloseRequested), Response::Exit),
            "the close button must actually close it"
        );
    }

    #[test]
    fn an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not() {
        let mut a = app();
        assert!(
            matches!(
                a.on_event(&Event::Key(probe::press(Key::M))),
                Response::Redraw
            ),
            "changing the mode needs redrawing"
        );
        assert!(
            matches!(
                a.on_event(&Event::Key(probe::press(Key::Q))),
                Response::Idle
            ),
            "a key that does nothing needs no frame"
        );
        assert!(
            matches!(a.on_event(&Event::FocusOut), Response::Idle),
            "and neither does losing focus"
        );
    }

    #[test]
    fn the_info_band_says_the_mode_the_tracking_the_filter_and_the_readings() {
        let mut a = app();
        to_mode(&mut a, MagnifyMode::Lens);
        to_tracking(&mut a, TrackingMode::Manual);
        to_filter(&mut a, ColorFilter::Greyscale);
        down(&mut a, Key::C);
        let line = a.info_line();
        for want in [
            MagnifyMode::Lens.label(),
            TrackingMode::Manual.label(),
            ColorFilter::Greyscale.label(),
        ] {
            assert!(
                line.contains(want),
                "the info line should say {want}: {line}"
            );
        }
        assert!(
            line.contains("0 shots"),
            "and how many shots were taken: {line}"
        );
        assert!(
            texts(&a.draw(SIZE)).contains(&line),
            "and the window should draw it"
        );
    }

    #[test]
    fn a_screenshot_is_counted_and_the_count_is_shown() {
        let mut a = app();
        ctrl_down(&mut a, Key::S);
        assert_eq!(a.shots(), 1);
        assert!(a.info_line().contains("1 shot"), "{}", a.info_line());
        assert!(
            !a.info_line().contains("1 shots"),
            "one shot is not plural: {}",
            a.info_line()
        );
        ctrl_down(&mut a, Key::S);
        assert!(a.info_line().contains("2 shots"), "{}", a.info_line());
    }

    #[test]
    fn the_header_says_the_zoom_and_says_paused_instead_when_it_is() {
        let mut a = app();
        down(&mut a, Key::Num9);
        let said = texts(&a.draw(SIZE));
        assert!(
            said.iter().any(|t| t == "15x"),
            "the header shows the zoom: {said:?}"
        );
        down(&mut a, Key::Escape);
        let said = texts(&a.draw(SIZE));
        assert!(
            said.iter().any(|t| t == "paused"),
            "and says paused instead when it is: {said:?}"
        );
    }

    #[test]
    fn the_crosshair_can_be_switched_off_and_the_button_shows_which_it_is() {
        let mut a = app();
        assert!(a.crosshair(), "on by default");
        assert_eq!(a.control_label(Target::ToggleCrosshair), "Cross \u{2713}");
        let with = lines(&a.draw(SIZE)).len();
        down(&mut a, Key::X);
        assert!(!a.crosshair());
        assert_eq!(a.control_label(Target::ToggleCrosshair), "Cross");
        let without = lines(&a.draw(SIZE)).len();
        assert!(without < with, "the crosshair should stop being drawn");
    }

    #[test]
    fn the_lens_shape_key_switches_between_a_circle_and_a_square() {
        let mut a = app();
        assert_eq!(a.lens_shape(), LensShape::Circle);
        down(&mut a, Key::L);
        assert_eq!(a.lens_shape(), LensShape::Rectangle);
        assert_eq!(a.lens_shape().label(), "Square");
        down(&mut a, Key::L);
        assert_eq!(a.lens_shape(), LensShape::Circle);
        assert!(
            a.lens_shape().radius(200.0, 200.0) > LensShape::Rectangle.radius(200.0, 200.0),
            "a circle is a rounded rectangle taken to its limit, so it is rounder"
        );
        assert_eq!(
            a.lens_shape().radius(200.0, 200.0),
            100.0,
            "which for a 200 square lens is a radius of 100"
        );
    }

    // ── The centring campaign ──────────────────────────────────────────────

    /// Window sizes the containment tests sweep.
    ///
    /// Chosen for the *bands they lose*, not for being round. `Layout::new`
    /// drops the header, then the controls, then the info line as the window
    /// shortens, so a list of comfortable sizes never reads a band that is not
    /// there; and the tall-and-narrow entries are the ones that keep every band
    /// while starving each of width, which is the other axis a centring can
    /// spill along.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 60.0),
        (160.0, 900.0),
        (200.0, 150.0),
        (240.0, 48.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (480.0, 700.0),
        (640.0, 500.0),
        (820.0, 620.0),
        (900.0, 260.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
    ];

    /// Bands narrower and shorter than `Layout::new` would ever hand out.
    ///
    /// A fixed sliver would only ever reach the outermost refusal. The failures
    /// that matter live in the narrow window between "one line fits" and "one
    /// line and the row under it fits", which is a fraction of the band rather
    /// than a constant — the info band's two-row stack and the paused overlay's
    /// two-line stack are each wrong for one particular height and right either
    /// side of it.
    fn squeezes(r: Rect) -> Vec<Rect> {
        let mut out = vec![r];
        let mut push_h = |h: f32| {
            if h < r.h {
                out.push(Rect::new(r.x, r.y, r.w, h));
            }
        };
        for h in [0.0, 1.0, 3.0, 6.0, 12.0, 24.0] {
            push_h(h);
        }
        for k in 1..16_u8 {
            push_h(r.h * f32::from(k) / 16.0);
        }
        let mut push_w = |w: f32| {
            if w < r.w {
                out.push(Rect::new(r.x, r.y, w, r.h));
            }
        };
        for w in [0.0, 1.0, 5.0, 30.0, 120.0] {
            push_w(w);
        }
        for k in 1..16_u8 {
            push_w(r.w * f32::from(k) / 16.0);
        }
        out
    }

    type Pass = fn(&Magnifier, &mut Frame, &Layout);
    type Band = fn(&mut Layout) -> &mut Rect;
    type Region = fn(&Layout) -> Rect;

    /// Every drawing pass and the region it owns.
    ///
    /// `help` owns the **window**, and that is not a mistake to be tidied away:
    /// the sheet scrims the whole screen, so the window really is the region it
    /// paints. The consequence is that this test says nothing whatever about
    /// the sheet's own panel, which is why
    /// `every_run_the_help_sheet_draws_stays_inside_its_panel` exists below.
    const PASSES: [(&str, Pass, Region); 5] = [
        ("viewport", Magnifier::draw_viewport, |l| l.viewport),
        ("header", Magnifier::draw_header, |l| l.header),
        ("info", Magnifier::draw_info, |l| l.info),
        ("controls", Magnifier::draw_controls, |l| l.controls),
        ("help", Magnifier::draw_help, |l| l.window),
    ];

    /// The bands a test may hand a box the layout would not.
    ///
    /// The viewport is squeezed through the window rather than directly: the
    /// panes, the lens and the ruler's reading are all *derived* from it inside
    /// the drawing, so a viewport replaced on its own is one the panes were
    /// never solved for and any overrun would be the test's doing.
    const SQUEEZABLE: [(&str, Band, Pass); 3] = [
        ("header", |l| &mut l.header, Magnifier::draw_header),
        ("info", |l| &mut l.info, Magnifier::draw_info),
        ("controls", |l| &mut l.controls, Magnifier::draw_controls),
    ];

    /// The screens, so every pass runs against a model with something to say as
    /// well as one without.
    fn states() -> Vec<(&'static str, Magnifier)> {
        let mut paused = app();
        paused.paused = true;

        let mut picked = app();
        picked.picked = Some((0x89, 0xB4, 0xFA));
        picked.status = "Picked #89B4FA".to_string();

        let mut measuring = app();
        measuring.ruler = Ruler::Done {
            start: (100.0, 100.0),
            end: (900.0, 700.0),
        };
        measuring.crosshair = true;

        let mut helping = app();
        helping.show_help = true;

        let mut quiet = app();
        quiet.status = String::new();

        vec![
            ("fresh", app()),
            ("paused", paused),
            ("with a colour picked", picked),
            ("with a measurement", measuring),
            ("with the shortcuts open", helping),
            ("with nothing to report", quiet),
        ]
    }

    /// Every filled rectangle, as the box it fills.
    ///
    /// Lines are deliberately not here. The crosshair's arms and the ruler's
    /// bar are drawn *through* their pane and cut off by a clip, which is what
    /// a crosshair is: a cross that runs to the edges. Text and fills are a
    /// different matter — a run cut off by a clip is a run that was placed
    /// wrongly and hidden — which is why the ruler's reading is cut to its pane
    /// in the drawing rather than left to the clip, and is checked here.
    fn painted(f: &Frame) -> Vec<Rect> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    /// Every run of type, as the box the renderer is entitled to fill.
    ///
    /// The height is `text::line_height` and not the font size: `push_text`
    /// puts the run's *top-left* corner where it is told, so a run occupies a
    /// full line height below `y`, and measuring it as the font size would let
    /// a band a hair too short pass a test of the very thing it refuses.
    fn inked(f: &Frame) -> Vec<(String, Rect)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    x,
                    y,
                    max_width,
                    font_size,
                    font_weight,
                    ..
                } => {
                    let w =
                        max_width.unwrap_or_else(|| text::measure(text, *font_size, *font_weight));
                    let h = text::line_height(*font_size, *font_weight);
                    Some((text.clone(), Rect::new(*x, *y, w, h)))
                }
                _ => None,
            })
            .collect()
    }

    /// A box with no area is inside anything: it is a thing that was not drawn.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    fn check_containment(state: &str, pass: &str, region: Rect, f: &Frame) {
        for r in painted(f) {
            assert!(
                inside(region, r),
                "{state}: the {pass} pass, given {region:?}, painted {r:?}"
            );
        }
        for (s, r) in inked(f) {
            assert!(
                inside(region, r),
                "{state}: the {pass} pass, given {region:?}, inked {s:?} at {r:?}"
            );
        }
        for (target, rect) in f.hits() {
            assert!(
                inside(region, *rect),
                "{state}: the {pass} pass, given {region:?}, hit-boxed {target:?} at {rect:?}"
            );
        }
    }

    #[test]
    fn centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative() {
        let band = Rect::new(10.0, 100.0, 80.0, 20.0);
        assert_eq!(
            centre_line(band, 20.0),
            Some(100.0),
            "a run exactly as tall as its band sits at the band's top"
        );
        assert_eq!(
            centre_line(band, 10.0),
            Some(105.0),
            "and one half as tall sits half the slack down"
        );
        assert_eq!(
            centre_line(band, 20.001),
            None,
            "a run taller than its band has nowhere to go, and the answer is \
             not a negative offset"
        );
        assert_eq!(
            centre_line(Rect::EMPTY, 0.0),
            None,
            "a band with no area holds nothing at all, not even a run of no height"
        );
        assert_eq!(
            centre_line(Rect::new(0.0, 0.0, 0.0, 50.0), 10.0),
            None,
            "and a band with height but no width is still a band with no area"
        );
    }

    #[test]
    fn no_pass_paints_outside_the_region_it_owns() {
        for &(w, h) in WINDOWS {
            for chrome in [true, false] {
                let l = Layout::new(w, h, chrome);
                for (state, a) in states() {
                    for (name, pass, region) in PASSES {
                        let mut f = Frame::new(w, h);
                        pass(&a, &mut f, &l);
                        check_containment(
                            &format!("{state} at {w}x{h} chrome={chrome}"),
                            name,
                            region(&l),
                            &f,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_pass_paints_outside_a_band_squeezed_below_anything_the_layout_hands_out() {
        // The window list alone is not enough. `Layout::new` gives every band a
        // height its own text fits, so a bound checked only against the layout's
        // own answers is a bound checked only where it was never going to bite.
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            for (state, a) in states() {
                for (name, band, pass) in SQUEEZABLE {
                    for squeezed in squeezes(*band(&mut base.clone())) {
                        let mut l = base;
                        *band(&mut l) = squeezed;
                        let mut f = Frame::new(w, h);
                        pass(&a, &mut f, &l);
                        check_containment(
                            &format!("{state} at {w}x{h} with {name} squeezed to {squeezed:?}"),
                            name,
                            squeezed,
                            &f,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_band_tall_enough_for_a_line_draws_one() {
        // The converse of the containment tests, and they are worthless without
        // it. Every bound this campaign added is a *refusal*, and a refusal
        // satisfies containment trivially: nothing is inside everything. A
        // program that drew no chrome at all would pass every test above.
        //
        // Height only, and not width. A band can legitimately be too narrow to
        // draw in — the header's title column collapses onto its left edge when
        // the reading takes the whole band, and `push_text` then refuses it,
        // which is the bound working rather than failing. There is no such
        // legitimate reason to refuse a band that is tall enough.
        let mut a = app();
        a.status = "Picked #89B4FA".to_string();
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            for (name, band, pass) in SQUEEZABLE {
                for squeezed in squeezes(*band(&mut base.clone())) {
                    let mut l = base;
                    *band(&mut l) = squeezed;
                    if !l.shows(squeezed) {
                        continue;
                    }
                    let mut f = Frame::new(w, h);
                    pass(&a, &mut f, &l);
                    assert!(
                        !inked(&f).is_empty() || !painted(&f).is_empty(),
                        "at {w}x{h} the {name} band was {squeezed:?}, which `shows` calls big \
                         enough to draw in, and the pass drew nothing at all"
                    );
                }
            }
        }
    }

    #[test]
    fn every_run_the_help_sheet_draws_stays_inside_its_panel() {
        // The sheet's pass owns the whole window — it scrims it, which is why
        // `PASSES` gives it `window` — so the containment test has nothing to
        // say about the panel, and a title hanging off its panel onto the scrim
        // is still inside the window. That is the hole this fills.
        //
        // Swept over squeezed windows as well as real ones, and for the same
        // reason `SQUEEZABLE` exists: at every size in `WINDOWS` the sheet is a
        // generous enough fraction of the window that its nominal offsets land
        // inside it, so a bound only those sizes exercise is a bound that has
        // not been exercised. Squeezing `window` is what shrinks the sheet,
        // since `help` is solved from it, while `pad` and the font sizes stay
        // as the real window set them — which is precisely the mismatch that
        // puts the title out.
        let mut a = app();
        a.show_help = true;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            let mut windows = vec![l.window];
            windows.extend(squeezes(l.window));
            for win in windows {
                let mut sq = l;
                sq.window = win;
                // The sheet is a fraction of the window it is solved from, so
                // squeezing the window is what makes the sheet too small for
                // the runs the *unsqueezed* font sizes put in it.
                let help_w = (win.w * 0.94).min(460.0);
                let help_h = (win.h * 0.94).min(400.0);
                sq.help = Rect::new(
                    win.x + (win.w - help_w) / 2.0,
                    win.y + (win.h - help_h) / 2.0,
                    help_w,
                    help_h,
                );
                let mut f = Frame::new(w, h);
                Magnifier::draw_help(&a, &mut f, &sq);
                for (text, r) in inked(&f) {
                    assert!(
                        inside(sq.help, r),
                        "at {w}x{h} with the window read as {win:?}, the sheet drew {text:?} \
                         at {r:?}, outside its panel at {:?}",
                        sq.help
                    );
                }
            }
        }
    }

    /// The sheet's own layout, rebuilt from the numbers `draw_help` uses.
    ///
    /// Repeated from the drawing code rather than factored out of it. A shared
    /// helper would make the tests below true by construction — they would be
    /// checking that a number equals itself — whereas a copy disagrees with the
    /// original the moment one of them changes, and disagreeing is the whole
    /// job. Same reasoning as automator's sidebar-row invariant.
    fn help_geometry(l: &Layout) -> (Rect, f32, f32, f32) {
        let h = l.help;
        let inner = Rect::new(h.x + l.pad * 1.5, h.y, h.w - l.pad * 3.0, h.h);
        let title = (l.big * 0.8).clamp(9.0, 18.0);
        let title_h = text::line_height(title, FontWeightHint::Bold);
        let top = h.y + l.pad * 2.0 + title_h;
        let foot = text::line_height(l.font * 0.8, FontWeightHint::Regular) + l.pad;
        let room = (h.bottom() - foot - top).max(0.0);
        #[expect(clippy::cast_precision_loss, reason = "fourteen rows; exact in f32")]
        let step = room / HELP_ROWS.len() as f32;
        (inner, top, step, foot)
    }

    /// Every sheet the suite can put on screen: the real window sizes, and the
    /// squeezed ones that shrink the sheet while leaving `pad` and the font
    /// sizes as the real window set them.
    ///
    /// That mismatch is deliberate and is what makes the sheet interesting —
    /// it is the case where the sheet is too small for the type it was asked to
    /// carry, which no unsqueezed size reaches.
    fn help_sheets() -> Vec<(f32, f32, Layout)> {
        let mut out = Vec::new();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            let mut windows = vec![l.window];
            windows.extend(squeezes(l.window));
            for win in windows {
                let mut sq = l;
                sq.window = win;
                let help_w = (win.w * 0.94).min(460.0);
                let help_h = (win.h * 0.94).min(400.0);
                sq.help = Rect::new(
                    win.x + (win.w - help_w) / 2.0,
                    win.y + (win.h - help_h) / 2.0,
                    help_w,
                    help_h,
                );
                out.push((w, h, sq));
            }
        }
        out
    }

    /// A row is drawn in a strip `step` points tall, and the type in it is
    /// sized *from* `step` — so when the sheet is short enough that `step` falls
    /// under the six-point floor, the line wants more room than the strip has
    /// and `centre_line` refuses the row rather than drawing it.
    ///
    /// Containment is structurally blind to that refusal, which is why this
    /// test is separate from the one above rather than another assertion inside
    /// it: a row drawn in a strip too short for it does not leave the sheet, it
    /// lands on top of its neighbour. Both runs are still inside the panel and
    /// both are unreadable. Overlap, not escape, is the fault here.
    #[test]
    fn no_two_help_rows_are_drawn_on_top_of_each_other() {
        let mut a = app();
        a.show_help = true;
        let keys: Vec<&str> = HELP_ROWS.iter().map(|(k, _)| *k).collect();
        let mut crowded = 0_u32;
        for (w, h, l) in help_sheets() {
            let mut f = Frame::new(w, h);
            Magnifier::draw_help(&a, &mut f, &l);
            let mut rows: Vec<Rect> = inked(&f)
                .into_iter()
                .filter(|(t, _)| keys.contains(&t.as_str()))
                .map(|(_, r)| r)
                .collect();
            rows.sort_by(|p, q| p.y.total_cmp(&q.y));
            let (_, _, step, _) = help_geometry(&l);
            // The sheets that would overlap if the refusal were dropped: the
            // strip is shorter than the line it is asked to hold. The first
            // draft of this counted sheets where such a strip had *drawn* a
            // row, which is a condition that can only hold when the refusal is
            // already broken — a coverage assertion that fails against correct
            // code and passes against faulty code, i.e. exactly backwards. What
            // is being covered is the input, not the outcome.
            let size = (step * 0.72).clamp(6.0, l.font);
            if step > 0.0 && step < text::line_height(size, FontWeightHint::Bold) {
                crowded += 1;
            }
            for pair in rows.windows(2) {
                let (prev, next) = (pair[0], pair[1]);
                assert!(
                    next.y >= prev.bottom() - 0.01,
                    "at {w}x{h} with the sheet at {:?}, a help row starting at {} runs into the \
                     one above it, which ends at {} — the strip is {step} tall and the line \
                     needs more",
                    l.help,
                    next.y,
                    prev.bottom()
                );
            }
        }
        // The refusal has to actually be reached, or this test is a sweep over
        // sheets that were all roomy enough and would pass against a version
        // with no refusal in it at all.
        assert!(
            crowded > 0,
            "no sheet in the sweep was short enough for its rows to want more room than \
             their strips, so nothing here exercised the refusal"
        );
    }

    /// The sheet keeps margins, and a run in the margin is a run outside the
    /// column it belongs to even though it is comfortably inside the panel.
    ///
    /// This is the tighter half of the containment test above, and it is what
    /// catches the split being computed from the wrong rectangle. Both columns
    /// are placed from one `split`, so a split taken from the *panel*'s width
    /// instead of the *column*'s moves the keys and the description together
    /// and they stay consistent with each other — the pair simply slides into
    /// the left margin. Measuring the pair against the panel cannot see that;
    /// measuring each run against the column can. A bound on a run is only as
    /// good as the box it is measured against, and here the box with the claim
    /// on it is the column, not the sheet.
    #[test]
    fn every_run_the_help_sheet_draws_stays_inside_its_inner_column() {
        let mut a = app();
        a.show_help = true;
        for (w, h, l) in help_sheets() {
            let (inner, ..) = help_geometry(&l);
            if inner.is_empty() {
                continue;
            }
            let mut f = Frame::new(w, h);
            Magnifier::draw_help(&a, &mut f, &l);
            for (text, r) in inked(&f) {
                assert!(
                    r.x >= inner.x - 0.01 && r.right() <= inner.right() + 0.01,
                    "at {w}x{h} the sheet drew {text:?} spanning {}..{}, outside its column at \
                     {}..{}",
                    r.x,
                    r.right(),
                    inner.x,
                    inner.right()
                );
            }
        }
    }

    /// The row rects are inside the sheet before `intersect` is applied to
    /// them, so the `intersect` never actually cuts one.
    ///
    /// That sounds like an argument for deleting it, and it is the opposite.
    /// A sweep deleting the `intersect` survives, so without this test the cut
    /// is a line no mutation can kill — and the reason it survives is not that
    /// it is pointless but that the rows are held inside by *arithmetic
    /// elsewhere*: `room` is measured down from `h.bottom()`, so `top + n·step`
    /// cannot pass the sheet's foot. The `intersect` is what makes the rows
    /// safe independently of that arithmetic, and this test is what makes the
    /// arithmetic itself checked. If an edit to `room` or `top` ever lets a row
    /// hang off the sheet, this fails and the `intersect` quietly starts doing
    /// work — which is exactly the moment someone should be told.
    ///
    /// Recorded as the reason the two `draw_help` refusal mutations are marked
    /// equivalent in `mutate.py` rather than left as open survivors.
    #[test]
    fn the_help_sheets_rows_are_inside_it_before_they_are_cut_to_it() {
        let mut checked = 0_u32;
        for (w, h, l) in help_sheets() {
            let (inner, top, step, _) = help_geometry(&l);
            if inner.is_empty() {
                continue;
            }
            for i in 0..HELP_ROWS.len() {
                #[expect(clippy::cast_precision_loss, reason = "fourteen rows; exact in f32")]
                let y = step.mul_add(i as f32, top);
                let row = Rect::new(inner.x, y, inner.w, step);
                if row.is_empty() {
                    continue;
                }
                checked += 1;
                // `inside`, not `intersect(..) == Some(row)`. The cut
                // recomputes the height as `bottom - y`, which for a row that
                // spans the sheet exactly returns a value half a millionth of a
                // point from the one it went in with — so exact equality here
                // reports a rounding step as a containment failure. What is
                // being asserted is that the cut takes nothing away, and a
                // tolerance is the honest way to say that about floats.
                assert!(
                    inside(l.help, row),
                    "at {w}x{h} help row {i} at {row:?} is not wholly inside the sheet at {:?}, \
                     so the cut that follows it is load-bearing after all",
                    l.help
                );
            }
        }
        assert!(
            checked > 0,
            "no sheet in the sweep produced a row at all, so nothing was checked"
        );
    }

    /// `push_text`'s three refusals, tested on `push_text` rather than through
    /// a window size that happens to reach them.
    ///
    /// The sweep test below is the one that was *meant* to cover the
    /// `limit <= 0.0` arm, and a mutation deleting that arm survived it. Not
    /// because the sweep is weak — because after the centring rewrite there is
    /// no longer any band in the layout that produces a limit of zero. Every
    /// column is now measured from a `Rect` that a refusal already emptied, so
    /// the arm is real but unreached, and a guard the suite cannot reach is a
    /// guard the suite cannot keep.
    ///
    /// Widening the layout sweep until it produced a zero limit would be
    /// testing the wrong thing: the arm is a promise `push_text` makes to every
    /// caller, present and future, not a fact about today's four bands. So it
    /// is tested where it is made. This is the general shape — when a sweep
    /// cannot reach a helper's guard, move the test to the helper instead of
    /// contorting the sweep, and leave the sweep asserting what it is good at.
    #[test]
    fn push_text_refuses_a_box_with_no_room_in_it() {
        let cases: [(&str, f32, &str, f32); 5] = [
            ("no width at all", 12.0, "Zoom 4x", 0.0),
            ("a width below zero", 12.0, "Zoom 4x", -3.0),
            ("no type size", 0.0, "Zoom 4x", 80.0),
            ("a type size below zero", -1.0, "Zoom 4x", 80.0),
            ("nothing to say", 12.0, "", 80.0),
        ];
        for (why, size, text_str, limit) in cases {
            let mut f = Frame::new(200.0, 100.0);
            push_text(
                &mut f,
                10.0,
                10.0,
                text_str,
                size,
                TEXT_COLOR,
                FontWeightHint::Regular,
                limit,
            );
            assert!(
                inked(&f).is_empty(),
                "given {why}, push_text drew {:?} anyway",
                inked(&f)
            );
        }
        // The other half of the biconditional. Without it the assertions above
        // are satisfied by a `push_text` that never draws anything at all, and
        // the mutation that empties the function would pass a test named for
        // guarding it.
        let mut f = Frame::new(200.0, 100.0);
        push_text(
            &mut f,
            10.0,
            10.0,
            "Zoom 4x",
            12.0,
            TEXT_COLOR,
            FontWeightHint::Regular,
            80.0,
        );
        assert_eq!(
            inked(&f).len(),
            1,
            "given a real box and a real string, push_text drew nothing"
        );
    }

    #[test]
    fn no_run_is_pushed_into_a_box_with_no_room() {
        // `push_text`'s `limit <= 0.0` refusal. A run given a limit of nothing
        // is ellipsised into nothing and drawn at a point no column contains,
        // and the guard is only *entered* where a column has been squeezed onto
        // its own left edge — which the window list never does, so the squeezed
        // sweep is what makes this test test anything at all.
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            for (state, a) in states() {
                for (_, band, pass) in SQUEEZABLE {
                    for squeezed in squeezes(*band(&mut base.clone())) {
                        let mut l = base;
                        *band(&mut l) = squeezed;
                        let mut f = Frame::new(w, h);
                        pass(&a, &mut f, &l);
                        for c in f.commands() {
                            if let RenderCommand::Text {
                                text, max_width, ..
                            } = c
                            {
                                assert!(
                                    max_width.is_some_and(|m| m > 0.0),
                                    "{state} at {w}x{h}: {text:?} was drawn with a limit of \
                                     {max_width:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_centred_string_is_stopped_at_the_right_hand_edge_of_its_box() {
        // A centred run is inset by half the slack, so a run handed the box's
        // full *width* as its limit may end half the slack past the box's right
        // edge — and the wider the string the further past. What a centred run
        // needs is the distance from where it starts to where the box ends.
        //
        // The buttons are the boxes this bites in: their text is centred in a
        // control that a narrow controls band divides into slivers, and the
        // labels are words like "Greyscale" that do not shrink with the band.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            let a = app();
            let mut f = Frame::new(w, h);
            a.draw_controls(&mut f, &l);
            for (slot, _) in CONTROLS.iter().enumerate() {
                let r = l.button(l.controls, slot, CONTROLS.len());
                if r.is_empty() {
                    continue;
                }
                for c in f.commands() {
                    if let RenderCommand::Text { x, max_width, .. } = c
                        && *x >= r.x - 0.01
                        && *x <= r.right() + 0.01
                    {
                        let limit = max_width.unwrap_or(f32::INFINITY);
                        assert!(
                            x + limit <= r.right() + 0.01,
                            "at {w}x{h} a run centred in {r:?} starts at {x} and is allowed \
                             {limit} more, which ends at {} — past the box",
                            x + limit
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_header_title_gives_way_to_the_reading_rather_than_running_under_it() {
        // Two flat fractions of the band — `0.5` for the title and `0.45` for
        // the reading — sum to less than one only because they were chosen to,
        // and neither knew what the other was drawing. A `pad` at each end plus
        // a reading wider than 45% of a narrow header put the two through each
        // other. The title's column ends where the reading's begins now, which
        // is one number rather than two guesses.
        //
        // Squeezed widths, not just the real ones. At every size in `WINDOWS`
        // the reading is far too short to reach a flat half of the band —
        // "paused" is the longest thing it ever says, and the widest header the
        // layout hands out is 1920 — so restoring the flat half changed nothing
        // any real window could see, and the row for it survived the sweep
        // saying so. What reaches the fault is a *narrow* band whose fonts were
        // sized by a *tall* window: `small` follows `header.h` and the window's
        // `big`, and neither of them shrinks when the width does. Height is left
        // alone here because it is the width the two columns are dividing.
        let mut a = app();
        a.paused = true;
        let mut narrow = 0;
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            for band in squeezes(base.header) {
                if band.h != base.header.h || !base.shows(band) {
                    continue;
                }
                let mut l = base;
                l.header = band;
                let mut f = Frame::new(w, h);
                a.draw_header(&mut f, &l);
                let runs = inked(&f);
                for (i, (a_text, a_rect)) in runs.iter().enumerate() {
                    for (b_text, b_rect) in runs.iter().skip(i + 1) {
                        assert!(
                            a_rect.intersect(*b_rect).is_none(),
                            "at {w}x{h} a {}-wide header drew {a_text:?} at {a_rect:?} and \
                             {b_text:?} at {b_rect:?}, which overlap",
                            band.w
                        );
                    }
                }
                // The coverage half of the claim. A no-overlap assertion over
                // bands where the reading could not have overlapped anything
                // is an assertion about nothing, and that is exactly the state
                // this test was in. So: count the bands narrow enough that a
                // flat half of them would have run the title into the reading,
                // and require some.
                if let Some((_, reading)) = runs.iter().find(|(t, _)| t == "paused")
                    && reading.x < band.x + l.pad + band.w * 0.5
                {
                    narrow += 1;
                }
            }
        }
        assert!(
            narrow > 0,
            "no header was ever narrow enough for a flat half of it to reach the \
             reading, so the split was never actually put to the question"
        );
    }

    /// The other half of the header's bounds: both centrings are refusals, and
    /// a refusal is invisible to a test that only asks where a run went. This
    /// asks whether it went anywhere.
    ///
    /// The reading's centring is the tight one. `small` has a `.max(8.0)`
    /// legibility floor, which is a literal that knows nothing about the band
    /// it is drawn in, and at the shortest header `shows` admits — 11px — an
    /// 8px line occupies 10.64 of them. It fits by a third of a pixel. That is
    /// a coincidence, not a margin, and the only thing that keeps it a fact is
    /// this assertion: move `shows`'s threshold, the floor, or the face's line
    /// height, and the reading silently stops being drawn rather than
    /// overflowing, which is the failure this campaign is least able to see.
    #[test]
    fn a_header_tall_enough_to_be_shown_always_says_what_the_zoom_is() {
        let mut a = app();
        a.paused = true;
        let mut checked = 0;
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            for band in squeezes(base.header) {
                // Width held at what the layout gave: a header can legitimately
                // be too narrow to draw the reading in, and that refusal is the
                // bound working. Height is the dimension with no such excuse.
                if band.w != base.header.w || !base.shows(band) {
                    continue;
                }
                let mut l = base;
                l.header = band;
                let mut f = Frame::new(w, h);
                a.draw_header(&mut f, &l);
                let runs = inked(&f);
                assert!(
                    runs.iter().any(|(t, _)| t == "paused"),
                    "at {w}x{h} a header {}px tall is tall enough for `shows` to draw in, \
                     and it drew {} run(s), none of them the reading",
                    band.h,
                    runs.len()
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no header height was ever exercised, so this asserted nothing"
        );
    }

    #[test]
    fn the_pause_notice_is_drawn_whole_or_not_at_all_and_never_outside_the_viewport() {
        // `draw_paused` is reached through the viewport pass, so the
        // containment tests do measure it — against `l.viewport`, which is the
        // box it centres in. But a centring is only unbounded when the box is
        // smaller than the thing being centred, and `Layout::new` never hands
        // out a viewport that small, so the bound is never *reached* there.
        // Squeezing the viewport is what reaches it. Squeezing it is sound here
        // where it is not in `SQUEEZABLE`, because this pass reads `l.viewport`
        // and nothing derived from it — no pane was solved for a box this test
        // then replaced.
        //
        // The second assertion is the one with the content in it. Both lines
        // used to be offsets from a single centring, so a viewport with room
        // for one drew "Magnifier paused" and swallowed the line that says how
        // to resume — a state report with its own remedy cut off, which is
        // worse than reporting nothing and leaving the button to say it. Both
        // lines, or neither.
        let mut a = app();
        a.paused = true;
        let mut whole = 0;
        let mut refused = 0;
        for &(w, h) in WINDOWS {
            let base = Layout::new(w, h, true);
            let mut boxes = vec![base.viewport];
            boxes.extend(squeezes(base.viewport));
            for v in boxes {
                let mut l = base;
                l.viewport = v;
                let mut f = Frame::new(w, h);
                a.draw_paused(&mut f, &l);
                let runs = inked(&f);
                for (text, r) in &runs {
                    assert!(
                        inside(v, *r),
                        "at {w}x{h} with the viewport read as {v:?}, the notice drew {text:?} \
                         at {r:?}"
                    );
                }
                assert!(
                    runs.len() == 2 || runs.is_empty(),
                    "at {w}x{h} with the viewport read as {v:?} the notice drew {} of its two \
                     lines rather than both or neither: {runs:?}",
                    runs.len()
                );
                // The `+ line_h * 0.4` that used to place the second line put
                // it 60% of a line inside the first at every size, which is
                // what made a pair of hand-tuned offsets look tuned. Stacked
                // properly the two cannot touch.
                if let [(t1, r1), (t2, r2)] = runs.as_slice() {
                    assert!(
                        r1.intersect(*r2).is_none(),
                        "at {w}x{h} with the viewport read as {v:?} the notice drew {t1:?} at \
                         {r1:?} and {t2:?} at {r2:?}, which overlap"
                    );
                }
                if runs.is_empty() {
                    refused += 1;
                } else {
                    whole += 1;
                }
            }
        }
        // Both halves, because "drawn only where it fits" is satisfied outright
        // by a program that never draws, and "always drawn" by one that never
        // refuses.
        assert!(
            whole > 0,
            "the notice was never drawn at any size, so `inside` was never asked anything"
        );
        assert!(
            refused > 0,
            "no viewport was ever too short for the notice, so the refusal is untested"
        );
    }

    #[test]
    fn the_rulers_reading_is_cut_to_the_pane_the_measurement_was_taken_in() {
        // The reading floats with the measurement — centred between the two
        // ends and a line above them — so all four of its edges can leave the
        // pane, and every one of them did. `Some(pane.w)` as a limit reaches
        // the pane's full *width* from wherever the reading happens to start,
        // and the `- line_h - 2.0` puts it above the pane whenever the
        // measurement is taken near the top. The clip hid all of it, and a clip
        // is not a bound any test reading the drawing commands can see.
        //
        // The ends are chosen to put the midpoint at each corner of the screen
        // as well as in the middle, since which edge the reading leaves depends
        // on where in the screen the measurement was taken.
        let mut a = app();
        a.crosshair = false;
        let mut cut = 0;
        let mut whole = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            for (start, end) in [
                ((0.0, 0.0), (40.0, 20.0)),
                ((1900.0, 1060.0), (1860.0, 1040.0)),
                ((100.0, 100.0), (900.0, 700.0)),
                ((960.0, 0.0), (980.0, 30.0)),
                ((0.0, 540.0), (30.0, 560.0)),
            ] {
                a.ruler = Ruler::Done { start, end };
                let pane = a.showing_pane(&l);
                if pane.is_empty() {
                    continue;
                }
                let mut f = Frame::new(w, h);
                a.draw_ruler(&mut f, pane);
                match inked(&f).first() {
                    Some((text, r)) => {
                        assert!(
                            inside(pane, *r),
                            "at {w}x{h} a measurement from {start:?} to {end:?} put the reading \
                             {text:?} at {r:?}, outside the pane at {pane:?}"
                        );
                        let size = (pane.h * 0.05).clamp(8.0, 13.0);
                        if r.w < text::measure(text, size, FontWeightHint::Bold) - 0.01 {
                            cut += 1;
                        } else {
                            whole += 1;
                        }
                    }
                    // Refused outright, which is the same bound answering with
                    // its other answer.
                    None => cut += 1,
                }
            }
        }
        assert!(
            whole > 0,
            "the reading was cut at every size, so a reading that fits was never checked"
        );
        assert!(
            cut > 0,
            "no measurement ever put the reading past an edge, so the cut is untested"
        );
    }

    /// The status was drawn by no window size at all, and every containment
    /// test passed throughout: a run that is never emitted is trivially inside
    /// every band it might have left. Containment is a test of where a thing
    /// goes, and it says nothing about whether the thing goes anywhere. So the
    /// band gets a second, blunter obligation — that what the app is told to
    /// say, it says — and it is asserted at the ordinary sizes rather than the
    /// squeezed ones, because the claim is about the app working, not about it
    /// refusing gracefully.
    #[test]
    fn a_status_worth_reporting_reaches_the_frame_at_every_ordinary_window_size() {
        let mut a = app();
        a.picked = Some((0x89, 0xB4, 0xFA));
        a.status = "#89B4FA at 100, 200".to_string();

        let mut shown = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            if !l.shows(l.info) {
                continue;
            }
            let mut f = Frame::new(w, h);
            a.draw_info(&mut f, &l);
            let runs = inked(&f);
            // Not `contains`: the reading and the status are different runs,
            // and a test that only asked whether the text appeared somewhere
            // would be satisfied by the reading alone.
            let status = runs.iter().find(|(t, _)| t == &a.status);
            assert!(
                status.is_some(),
                "{w}x{h}: the info band drew {} run(s) and none of them was the status {:?}",
                runs.len(),
                a.status
            );
            if let Some((_, r)) = status {
                assert!(
                    inside(l.info, *r),
                    "{w}x{h}: the status went outside the info band: {r:?} vs {:?}",
                    l.info
                );
                let reading = runs
                    .iter()
                    .find(|(t, _)| t == &a.info_line())
                    .expect("the reading is drawn whenever the band is shown");
                // Not merely `intersect().is_none()`. Two runs that abut
                // exactly do not intersect and still read as one word, so the
                // contract is a *gap*, and it has to be asserted as one or the
                // padding that separates them is free to go to zero unnoticed.
                assert!(
                    reading.1.right() <= r.x - l.pad + 0.01,
                    "{w}x{h}: only {} of clear space between the reading and the \
                     status, which is under the {} padding that should separate them",
                    r.x - reading.1.right(),
                    l.pad
                );
                // The status is the smaller of the two runs, and it is centred
                // in the taller one's line box rather than hung from its top:
                // sharing a row means sharing a centre.
                assert!(
                    (reading.1.y + reading.1.h / 2.0 - (r.y + r.h / 2.0)).abs() < 0.01,
                    "{w}x{h}: the status is not centred on the reading it shares \
                     the row with: {r:?} vs {:?}",
                    reading.1
                );
            }
            shown += 1;
        }
        assert!(
            shown > 0,
            "no window in the grid showed an info band, so this asserted nothing"
        );
    }

    /// The status is capped at half the run so that it cannot crowd out the
    /// standing state it annotates. A cap is only a cap where something
    /// reaches it, so this asserts a message far too long to fit is *cut* and
    /// the reading still gets its half.
    #[test]
    fn a_long_status_is_cut_rather_than_taking_the_readings_half_of_the_row() {
        let mut a = app();
        a.status = "x".repeat(400);

        let mut cut = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, true);
            if !l.shows(l.info) {
                continue;
            }
            let mut f = Frame::new(w, h);
            a.draw_info(&mut f, &l);
            let runs = inked(&f);
            let Some((_, status)) = runs.iter().find(|(t, _)| t == &a.status) else {
                continue;
            };
            let Some((_, reading)) = runs.iter().find(|(t, _)| t == &a.info_line()) else {
                continue;
            };
            assert!(
                inside(l.info, *status),
                "{w}x{h}: an overlong status left the band: {status:?}"
            );
            assert!(
                reading.w >= status.w - 0.01,
                "{w}x{h}: a 400-character status took more of the row than the \
                 reading: status {} wide, reading {} wide",
                status.w,
                reading.w
            );
            cut += 1;
        }
        assert!(
            cut > 0,
            "the overlong status was never drawn, so the cap went unexercised"
        );
    }

    // __TESTS_TAIL__
}
