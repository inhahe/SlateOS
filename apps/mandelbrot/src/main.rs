#![allow(clippy::similar_names)]

//! Mandelbrot set explorer for SlateOS.
//!
//! Pan with the arrows, zoom with `Z`/`X`, `+`/`-` or the wheel, click to
//! centre, `C` to recolour, `R` to reset, `F1` for help.
//!
//! Six things were wrong with it before it had a window, and all six were
//! invisible while `main` built the app and dropped it:
//!
//! * **The click handler invented a window size.** It mapped the click through
//!   a hard-coded 800x600 — with a comment saying so — so the headline
//!   interaction, "click to centre", centred on the wrong point in every window
//!   that was not exactly 800x600, and the error grew with the discrepancy.
//! * **Every key fired twice.** `event` never read `KeyEvent::pressed`, so the
//!   press and the release each ran the action. `Z` zoomed twice per keystroke,
//!   and the two toggles — `I` for the info bar and `F1` for help — toggled
//!   back on release, so the help screen could not be opened at all.
//! * **The drawing and the clicking disagreed about where a point was.** The
//!   renderer indexed cells (`px / cols`), the click mapped pixels
//!   (`x / width`), and `cols` was a truncating divide — so the image was drawn
//!   stretched against the coordinate system used to interpret a click, and the
//!   right and bottom edges were left unpainted.
//! * **A frame cost an unbounded amount of arithmetic.** Cells times iterations
//!   with no ceiling: a maximised window at the highest resolution and the 2000
//!   iteration cap is four billion escape iterations, per frame, on the thread
//!   that is supposed to be answering the keyboard.
//! * **Every frame recomputed the fractal from scratch** — including frames
//!   where nothing about the view had changed and the only difference was that
//!   an overlay had been toggled.
//! * **The wheel did nothing**, though the documentation promised it zoomed.
//!
//! The escape counts are now cached, keyed by the view and the window; the
//! colour scheme is deliberately *not* part of that key, because recolouring a
//! cached grid is free, which is why `C` is instant even at 2000 iterations.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::cell::RefCell;
use std::process::ExitCode;

// ── Catppuccin Mocha ────────────────────────────────────────────────
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);

const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;

/// The most escape iterations one recomputation is allowed to spend.
///
/// Cells times iterations is otherwise unbounded: a maximised window at one
/// pixel per cell and the 2000-iteration cap is four billion iterations, on the
/// thread that also answers the keyboard. When the requested resolution would
/// exceed this the cells are made bigger until it does not, and the readout
/// says so rather than quietly ignoring the setting.
///
/// The figure is generous because the cache means it is paid when the view
/// moves, not once a frame: an idle window, a recolour, or an overlay toggle
/// all cost nothing. Twelve million is a few tens of milliseconds, which is a
/// pan that feels immediate rather than one that feels stuck.
const FRAME_ITER_BUDGET: u64 = 12_000_000;

/// The coarsest the budget may make a cell before it gives up and accepts a
/// slow frame. A window big enough to need cells larger than this is not
/// showing a fractal any more.
const MAX_CELL: f32 = 64.0;

/// `f64` runs out of mantissa somewhere near here: below it, neighbouring
/// cells map to the same complex number and the picture is noise rather than
/// detail. Refusing to zoom further shows the deepest honest image instead.
const MIN_SCALE: f64 = 1e-13;

/// Zoomed out this far the whole set is a speck; there is nothing past it.
const MAX_SCALE: f64 = 8.0;

const ZOOM_STEP: f64 = 0.7;

// ── Color schemes ───────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorScheme {
    Classic,
    Fire,
    Ocean,
    Neon,
    Grayscale,
}

impl ColorScheme {
    #[cfg(test)]
    const ALL: &[ColorScheme] = &[
        ColorScheme::Classic,
        ColorScheme::Fire,
        ColorScheme::Ocean,
        ColorScheme::Neon,
        ColorScheme::Grayscale,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Fire => "Fire",
            Self::Ocean => "Ocean",
            Self::Neon => "Neon",
            Self::Grayscale => "Grayscale",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Classic => Self::Fire,
            Self::Fire => Self::Ocean,
            Self::Ocean => Self::Neon,
            Self::Neon => Self::Grayscale,
            Self::Grayscale => Self::Classic,
        }
    }

    /// Map an escape iteration count to a colour.
    fn color(self, iter: u32, max_iter: u32) -> Color {
        if iter >= max_iter {
            return Color::from_hex(0x000000); // Black for points in the set
        }

        let t = f64::from(iter) / f64::from(max_iter.max(1));

        match self {
            Self::Classic => {
                let hue = (t * 360.0) % 360.0;
                hsv_to_color(hue, 1.0, 1.0)
            }
            Self::Fire => {
                let r = (t * 3.0).min(1.0);
                let g = ((t - 0.33).max(0.0) * 3.0).min(1.0);
                let b = ((t - 0.66).max(0.0) * 3.0).min(1.0);
                Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
            }
            Self::Ocean => {
                let r = ((t * 2.0 - 0.5).clamp(0.0, 1.0) * 100.0) as u8;
                let g = ((t * 1.5).min(1.0) * 200.0) as u8;
                let b = (t.min(1.0) * 255.0) as u8;
                Color::rgb(r, g, b)
            }
            Self::Neon => {
                let phase = t * core::f64::consts::TAU;
                // RGB channels offset by thirds of a full turn for a smooth rainbow.
                let third = core::f64::consts::TAU / 3.0;
                let r = ((phase.sin() * 0.5 + 0.5) * 255.0) as u8;
                let g = (((phase + third).sin() * 0.5 + 0.5) * 255.0) as u8;
                let b = (((phase + 2.0 * third).sin() * 0.5 + 0.5) * 255.0) as u8;
                Color::rgb(r, g, b)
            }
            Self::Grayscale => {
                let v = (t * 255.0) as u8;
                Color::rgb(v, v, v)
            }
        }
    }
}

fn hsv_to_color(h: f64, s: f64, v: f64) -> Color {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color::rgb(
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

// ── Mandelbrot computation ──────────────────────────────────────────

/// Escape iteration for the point (`cx`, `cy`) of the complex plane.
///
/// Returns the iteration at which |z| first exceeds 2, or `max_iter` if it
/// never does within the budget — which is the convention the colourists rely
/// on to paint the interior black.
fn mandelbrot_iter(cx: f64, cy: f64, max_iter: u32) -> u32 {
    let mut zx = 0.0_f64;
    let mut zy = 0.0_f64;
    let mut i = 0u32;

    while i < max_iter {
        let zx2 = zx * zx;
        let zy2 = zy * zy;
        if zx2 + zy2 > 4.0 {
            return i;
        }
        zy = 2.0 * zx * zy + cy;
        zx = zx2 - zy2 + cx;
        i = i.saturating_add(1);
    }
    max_iter
}

// ── Commands ────────────────────────────────────────────────────────

/// Everything the explorer can be told to do, from a key or from a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ZoomIn,
    ZoomOut,
    Reset,
    CycleScheme,
    ToggleHelp,
}

/// The footer buttons, in the order they are drawn. Each label names the key
/// that does the same thing, so the buttons double as the key legend the
/// bottom bar used to be.
const BUTTONS: [(Action, &str); 5] = [
    (Action::ZoomOut, "\u{2212}  Out"),
    (Action::ZoomIn, "+  In"),
    (Action::Reset, "R  Reset"),
    (Action::CycleScheme, "C  Colour"),
    (Action::ToggleHelp, "F1  Help"),
];

/// Everything a click can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The fractal itself: a click centres on the point under the pointer.
    Fractal,
    Button(Action),
    /// The help sheet, which swallows clicks and closes on one.
    Help,
}

pub type Frame = guitk::frame::Frame<Target>;

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes in a window of a given size.
///
/// Derived on every frame and never stored, so there is no second copy to fall
/// out of step with the window the compositor is actually showing.
pub struct Layout {
    pub window: Rect,
    /// The fractal fills the window; the bars are drawn over it. That is what
    /// makes a click anywhere in the window a fractal coordinate, which is what
    /// the maths assumes.
    pub fractal: Rect,
    pub info: Rect,
    pub footer: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
}

impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(0.0);
        let h = height.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);

        let font = (h / 40.0).clamp(8.0, 12.0);
        let small = (font - 2.0).max(7.0);

        let info_h = (h * 0.05).clamp(18.0, 26.0).min(h);
        let foot_h = (h * 0.05).clamp(18.0, 26.0).min((h - info_h).max(0.0));

        let help_w = (w * 0.9).min(360.0);
        let help_h = (h * 0.9).min(320.0);

        Self {
            window,
            fractal: window,
            info: Rect::new(0.0, 0.0, w, info_h),
            footer: Rect::new(0.0, h - foot_h, w, foot_h),
            help: Rect::new(
                (w - help_w) / 2.0,
                (h - help_h) / 2.0,
                help_w.max(0.0),
                help_h.max(0.0),
            ),
            font,
            small,
        }
    }

    /// The rectangle of footer button `index`, laid out from the right so that
    /// a narrow window loses the leftmost buttons rather than clipping them all.
    pub fn button(&self, index: usize) -> Rect {
        let gap = (self.footer.w * 0.01).min(6.0);
        let bw = ((self.footer.w - gap * 6.0) / 5.0).clamp(0.0, 96.0);
        let total = bw * 5.0 + gap * 4.0;
        let x0 = self.footer.x + (self.footer.w - total).max(0.0) / 2.0;
        Rect::new(
            x0 + index as f32 * (bw + gap),
            self.footer.y + 1.0,
            bw,
            (self.footer.h - 2.0).max(0.0),
        )
    }

    /// Whether there is room to draw the buttons at all.
    pub fn shows_buttons(&self) -> bool {
        self.footer.w >= 200.0 && self.footer.h >= 12.0
    }
}

// ── Cached escape counts ────────────────────────────────────────────

/// What a cached grid was computed for.
///
/// The colour scheme is deliberately absent: the grid holds iteration counts,
/// not colours, so recolouring is a repaint of cached data rather than a
/// recomputation. That is the difference between `C` being instant and `C`
/// costing three million escape iterations.
#[derive(Clone, Copy, PartialEq)]
struct TileKey {
    center_x: f64,
    center_y: f64,
    scale: f64,
    max_iter: u32,
    width: f32,
    height: f32,
    cell: f32,
}

struct Tile {
    key: TileKey,
    cols: usize,
    /// Row-major escape counts, `cols` per row. The row count is
    /// `iters.len() / cols` and is not stored, so it cannot disagree.
    iters: Vec<u32>,
}

// ── App ─────────────────────────────────────────────────────────────

pub struct MandelbrotApp {
    center_x: f64,
    center_y: f64,
    /// Width of the view in complex-plane units.
    scale: f64,
    max_iter: u32,
    color_scheme: ColorScheme,
    /// Requested pixels per fractal cell. The budget may enlarge it; see
    /// [`FRAME_ITER_BUDGET`].
    pixel_size: f32,
    show_info: bool,
    show_help: bool,
    /// The live window size, so that a click arriving between frames is
    /// interpreted against the same window the player is looking at. This is
    /// the field whose absence made the old click handler guess 800x600.
    width: f32,
    height: f32,
    /// Escape counts for the last view drawn. `RefCell` because rendering
    /// takes `&self` — the cache is an optimisation, not state the caller can
    /// observe, so it does not belong in the `&mut` half of the interface.
    tile: RefCell<Option<Tile>>,
    /// How many times the grid has actually been computed. The cache is the
    /// difference between `C` costing nothing and `C` costing a full frame of
    /// arithmetic, so it is worth a test -- and a cache can only be tested by
    /// counting the misses.
    #[cfg(test)]
    computes: std::cell::Cell<u32>,
}

impl MandelbrotApp {
    pub fn new() -> Self {
        Self {
            center_x: -0.5,
            center_y: 0.0,
            scale: 3.5,
            max_iter: 100,
            color_scheme: ColorScheme::Classic,
            pixel_size: 4.0,
            show_info: true,
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            tile: RefCell::new(None),
            #[cfg(test)]
            computes: std::cell::Cell::new(0),
        }
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    // ── View ────────────────────────────────────────────────────────

    fn reset_view(&mut self) {
        self.center_x = -0.5;
        self.center_y = 0.0;
        self.scale = 3.5;
        self.max_iter = 100;
    }

    fn zoom_in(&mut self) {
        self.scale = (self.scale * ZOOM_STEP).max(MIN_SCALE);
    }

    fn zoom_out(&mut self) {
        self.scale = (self.scale / ZOOM_STEP).min(MAX_SCALE);
    }

    /// Zoom keeping the complex point under (`sx`, `sy`) where it is.
    ///
    /// Zooming about the centre while the pointer is elsewhere walks the thing
    /// you are looking at off the screen, which is why every map does it this
    /// way. Written as "where was this pixel before, where is it now, shift by
    /// the difference" so it stays correct however the mapping changes.
    fn zoom_at(&mut self, sx: f32, sy: f32, zoom_in: bool) {
        let (bx, by) = self.complex_at(sx, sy);
        if zoom_in {
            self.zoom_in();
        } else {
            self.zoom_out();
        }
        let (ax, ay) = self.complex_at(sx, sy);
        self.center_x += bx - ax;
        self.center_y += by - ay;
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        self.center_x += dx * self.scale * 0.1;
        self.center_y += dy * self.scale * 0.1;
    }

    fn increase_iterations(&mut self) {
        self.max_iter = self.max_iter.saturating_add(50).min(2000);
    }

    fn decrease_iterations(&mut self) {
        self.max_iter = self.max_iter.saturating_sub(50).max(25);
    }

    fn increase_resolution(&mut self) {
        if self.pixel_size > 1.0 {
            self.pixel_size -= 1.0;
        }
    }

    fn decrease_resolution(&mut self) {
        if self.pixel_size < 16.0 {
            self.pixel_size += 1.0;
        }
    }

    fn goto(&mut self, cx: f64, cy: f64, scale: f64, max_iter: u32) {
        self.center_x = cx;
        self.center_y = cy;
        self.scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        self.max_iter = max_iter;
    }

    // ── Coordinates ─────────────────────────────────────────────────

    /// The complex number drawn at screen pixel (`sx`, `sy`).
    ///
    /// One function, used by the renderer and by the click handler both, so
    /// that clicking a feature centres on *that* feature. They used to be two
    /// expressions that did not quite agree.
    fn complex_at(&self, sx: f32, sy: f32) -> (f64, f64) {
        Self::complex_in(
            self.center_x,
            self.center_y,
            self.scale,
            sx,
            sy,
            self.width,
            self.height,
        )
    }

    fn complex_in(
        center_x: f64,
        center_y: f64,
        scale: f64,
        sx: f32,
        sy: f32,
        width: f32,
        height: f32,
    ) -> (f64, f64) {
        let w = f64::from(width.max(1.0));
        let h = f64::from(height.max(1.0));
        let aspect = w / h;
        (
            center_x + (f64::from(sx) / w - 0.5) * scale * aspect,
            center_y + (f64::from(sy) / h - 0.5) * scale,
        )
    }

    /// The cell size a frame will actually use, which is the requested one
    /// enlarged until the frame fits inside [`FRAME_ITER_BUDGET`].
    fn effective_cell(&self, width: f32, height: f32) -> f32 {
        let mut cell = self.pixel_size.max(1.0);
        loop {
            let cols = (width.max(1.0) / cell).ceil().max(1.0) as u64;
            let rows = (height.max(1.0) / cell).ceil().max(1.0) as u64;
            let work = cols
                .saturating_mul(rows)
                .saturating_mul(u64::from(self.max_iter));
            if work <= FRAME_ITER_BUDGET || cell >= MAX_CELL {
                return cell;
            }
            cell += 1.0;
        }
    }

    /// Whether the budget is currently overriding the requested resolution.
    fn resolution_capped(&self) -> bool {
        self.effective_cell(self.width, self.height) > self.pixel_size.max(1.0)
    }
}

impl Default for MandelbrotApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Escape-count grid ───────────────────────────────────────────────

impl MandelbrotApp {
    /// Compute the escape counts for `key`, one per cell.
    ///
    /// Each cell is sampled at its own centre rather than its corner, so the
    /// colour shown at a pixel is the colour of the point nearest that pixel —
    /// which is what makes clicking a feature land on the feature.
    fn compute_tile(key: TileKey, cell: f32) -> Tile {
        let cols = (key.width.max(1.0) / cell).ceil().max(1.0) as usize;
        let rows = (key.height.max(1.0) / cell).ceil().max(1.0) as usize;
        let mut iters = Vec::with_capacity(cols.saturating_mul(rows));
        let half = cell / 2.0;
        for row in 0..rows {
            let sy = row as f32 * cell + half;
            for col in 0..cols {
                let sx = col as f32 * cell + half;
                let (cx, cy) = Self::complex_in(
                    key.center_x,
                    key.center_y,
                    key.scale,
                    sx,
                    sy,
                    key.width,
                    key.height,
                );
                iters.push(mandelbrot_iter(cx, cy, key.max_iter));
            }
        }
        Tile { key, cols, iters }
    }

    /// Bring the cache up to date for a window of this size, and report the
    /// cell size the grid was built at.
    fn ensure_tile(&self, width: f32, height: f32) -> f32 {
        let cell = self.effective_cell(width, height);
        let key = TileKey {
            center_x: self.center_x,
            center_y: self.center_y,
            scale: self.scale,
            max_iter: self.max_iter,
            width,
            height,
            cell,
        };
        let mut slot = self.tile.borrow_mut();
        let fresh = slot.as_ref().is_some_and(|t| t.key == key);
        if !fresh {
            *slot = Some(Self::compute_tile(key, cell));
            #[cfg(test)]
            self.computes.set(self.computes.get().saturating_add(1));
        }
        cell
    }
}

// ── Commands ────────────────────────────────────────────────────────

impl MandelbrotApp {
    /// Whether `action` can do anything right now.
    ///
    /// A button that cannot act is drawn dim, but still records its hit box, so
    /// that a click on it stops there rather than falling through and recentring
    /// the fractal on the button the user was aiming at.
    fn enabled(&self, action: Action) -> bool {
        match action {
            Action::ZoomIn => self.scale > MIN_SCALE,
            Action::ZoomOut => self.scale < MAX_SCALE,
            Action::Reset | Action::CycleScheme | Action::ToggleHelp => true,
        }
    }

    /// Perform `action`. Returns whether anything changed, which is what tells
    /// the compositor whether this event was worth a repaint.
    fn apply(&mut self, action: Action) -> bool {
        if !self.enabled(action) {
            return false;
        }
        match action {
            Action::ZoomIn => self.zoom_in(),
            Action::ZoomOut => self.zoom_out(),
            Action::Reset => self.reset_view(),
            Action::CycleScheme => self.color_scheme = self.color_scheme.next(),
            Action::ToggleHelp => self.show_help = !self.show_help,
        }
        true
    }
}

// ── Input ───────────────────────────────────────────────────────────

impl MandelbrotApp {
    fn handle_key(&mut self, ke: &KeyEvent) -> EventResult {
        // Chorded keys belong to the desktop: Ctrl-R is not "reset view".
        if ke.modifiers.ctrl || ke.modifiers.alt || ke.modifiers.super_key {
            return EventResult::Ignored;
        }
        // The release of a key is not a second press of it. Acting on both is
        // what made `Z` zoom twice and made the two toggles cancel themselves
        // out before anything was ever drawn.
        if !ke.pressed {
            return EventResult::Ignored;
        }

        // While the help sheet is up it has the keyboard: panning a view you
        // cannot see is not a feature.
        if self.show_help {
            return match ke.key {
                Key::F1 | Key::Escape | Key::Enter | Key::Space => {
                    self.show_help = false;
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match ke.key {
            Key::Up => self.pan(0.0, -1.0),
            Key::Down => self.pan(0.0, 1.0),
            Key::Left => self.pan(-1.0, 0.0),
            Key::Right => self.pan(1.0, 0.0),
            Key::Z | Key::Equals => self.zoom_in(),
            Key::X | Key::Minus => self.zoom_out(),
            Key::R => self.reset_view(),
            Key::C => self.color_scheme = self.color_scheme.next(),
            Key::I => self.show_info = !self.show_info,
            Key::F1 => self.show_help = true,
            Key::Num1 => self.increase_iterations(),
            Key::Num2 => self.decrease_iterations(),
            Key::Num3 => self.increase_resolution(),
            Key::Num4 => self.decrease_resolution(),
            // Presets. The scale each one wants is inside the honest range, so
            // `goto`'s clamp never bites here -- it is there for the ones a
            // future edit adds.
            Key::F2 => self.goto(-0.745, 0.186, 0.01, 300), // Seahorse valley
            Key::F3 => self.goto(0.281_717, 0.5771, 0.005, 300), // Elephant valley
            Key::F4 => self.goto(-1.7497, 0.0, 0.02, 500),  // Mini Mandelbrot
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// What a click at (`x`, `y`) would land on, judged against the frame the
    /// user is actually looking at.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    fn handle_mouse(&mut self, me: &MouseEvent) -> EventResult {
        match me.kind {
            MouseEventKind::Scroll { dy, .. } => {
                if dy == 0.0 || self.show_help {
                    return EventResult::Ignored;
                }
                self.zoom_at(me.x, me.y, dy > 0.0);
                EventResult::Consumed
            }
            MouseEventKind::Press(MouseButton::Left) => match self.target_at(me.x, me.y) {
                Some(Target::Help) => {
                    self.show_help = false;
                    EventResult::Consumed
                }
                Some(Target::Button(action)) => {
                    self.apply(action);
                    // The click was spent on the button either way: a dim
                    // button absorbs it rather than passing it to the fractal
                    // underneath.
                    EventResult::Consumed
                }
                Some(Target::Fractal) => {
                    // Centre on the point clicked, then close in on it. This is
                    // the interaction that was mapping through a hard-coded
                    // 800x600 window and so missed by more the further the real
                    // window was from that size.
                    let (cx, cy) = self.complex_at(me.x, me.y);
                    self.center_x = cx;
                    self.center_y = cy;
                    self.zoom_in();
                    EventResult::Consumed
                }
                None => EventResult::Ignored,
            },
            _ => EventResult::Ignored,
        }
    }
}

// ── Rendering ───────────────────────────────────────────────────────

impl MandelbrotApp {
    /// Draw one frame, recording as it goes what each part of it can be
    /// clicked to do.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(width, height);

        fill(&mut f, l.window, Color::from_hex(0x000000));
        self.draw_fractal(&mut f, &l);
        // Recorded before the buttons, because `hit_test` searches backwards
        // and so the thing drawn last wins.
        f.hit(Target::Fractal, l.fractal);

        if self.show_info {
            self.draw_info(&mut f, &l);
        }
        self.draw_footer(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_fractal(&self, f: &mut Frame, l: &Layout) {
        let cell = self.ensure_tile(l.window.w, l.window.h);
        let slot = self.tile.borrow();
        let Some(tile) = slot.as_ref() else {
            return;
        };
        if tile.cols == 0 {
            return;
        }
        for (row, line) in tile.iters.chunks(tile.cols).enumerate() {
            let y = row as f32 * cell;
            for (col, &iter) in line.iter().enumerate() {
                f.push(RenderCommand::FillRect {
                    x: col as f32 * cell,
                    y,
                    width: cell,
                    height: cell,
                    color: self.color_scheme.color(iter, self.max_iter),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    /// The one-line readout of where the view is.
    fn info_text(&self) -> String {
        let capped = if self.resolution_capped() {
            format!(
                " (drawn at {:.0}px to fit the frame)",
                self.effective_cell(self.width, self.height)
            )
        } else {
            String::new()
        };
        format!(
            "Center: ({:.6}, {:.6})  Scale: {:.2e}  Iter: {}  Scheme: {}  Res: {:.0}px{}",
            self.center_x,
            self.center_y,
            self.scale,
            self.max_iter,
            self.color_scheme.name(),
            self.pixel_size,
            capped
        )
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.info, Color::rgba(0, 0, 0, 180));
        let pad = 8.0_f32.min(l.info.w / 4.0);
        label(
            f,
            l.info.x + pad,
            l.info.y + (l.info.h - text::line_height(l.font, FontWeightHint::Regular)) / 2.0,
            &self.info_text(),
            l.font,
            COL_TEXT,
            FontWeightHint::Regular,
            // The readout is long and the window may be narrow. Letting the
            // toolkit elide it keeps the leading coordinates, which are the
            // part worth reading, instead of clipping mid-digit.
            Some((l.info.w - pad * 2.0).max(0.0)),
        );
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.footer, Color::rgba(0, 0, 0, 180));
        if !l.shows_buttons() {
            return;
        }
        for (index, (action, text_label)) in BUTTONS.iter().enumerate() {
            let r = l.button(index);
            if r.w <= 0.0 {
                continue;
            }
            let on = self.enabled(*action);
            f.push(RenderCommand::FillRect {
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                color: if on {
                    Color::rgba(255, 255, 255, 24)
                } else {
                    Color::rgba(255, 255, 255, 10)
                },
                corner_radii: CornerRadii::all((r.h / 3.0).min(6.0)),
            });
            let (_, cy) = r.centre();
            centred_in(
                f,
                r.x,
                r.w,
                cy,
                text_label,
                l.small,
                if on { COL_TEXT } else { COL_OVERLAY0 },
                FontWeightHint::Regular,
            );
            // Recorded whether or not it can act, so that a click on a dim
            // button stops here instead of recentring the fractal.
            f.hit(Target::Button(*action), r);
        }
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 200));
        let p = l.help;
        if p.w <= 0.0 || p.h <= 0.0 {
            return;
        }
        f.push(RenderCommand::FillRect {
            x: p.x,
            y: p.y,
            width: p.w,
            height: p.h,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all((p.w / 30.0).min(12.0)),
        });

        let title = (l.font * 1.5).min(p.h / 4.0);
        let line = text::line_height(l.small, FontWeightHint::Regular).max(1.0);
        let pad = (p.w * 0.06).min(24.0);
        centred_in(
            f,
            p.x + pad,
            (p.w - pad * 2.0).max(0.0),
            p.y + title,
            "Mandelbrot Explorer",
            title,
            COL_TEXT,
            FontWeightHint::Bold,
        );

        let top = p.y + title * 2.0;
        // Only as many rows as fit. A help sheet that runs off its own panel is
        // worse than a short one, because the reader cannot tell it did.
        let room = (p.bottom() - pad - top).max(0.0);
        let fits = (room / line) as usize;
        let key_x = p.x + pad;
        let desc_x = p.x + (p.w * 0.42).min(p.w - pad);
        for (i, (key_name, desc)) in HELP_ROWS.iter().take(fits).enumerate() {
            let y = top + i as f32 * line;
            label(
                f,
                key_x,
                y,
                key_name,
                l.small,
                COL_BLUE,
                FontWeightHint::Bold,
                Some((desc_x - key_x).max(0.0)),
            );
            label(
                f,
                desc_x,
                y,
                desc,
                l.small,
                COL_SUBTEXT0,
                FontWeightHint::Regular,
                Some((p.right() - pad - desc_x).max(0.0)),
            );
        }

        // The whole window, so that a click anywhere dismisses the sheet --
        // including on the buttons it is covering, which would otherwise act
        // through it.
        f.hit(Target::Help, l.window);
    }
}

/// The key legend, shown by `F1`.
const HELP_ROWS: [(&str, &str); 15] = [
    ("Arrow keys", "Pan the view"),
    ("Z  or  +", "Zoom in"),
    ("X  or  \u{2212}", "Zoom out"),
    ("Wheel", "Zoom about the pointer"),
    ("Click", "Centre there and zoom in"),
    ("R", "Reset the view"),
    ("C", "Next colour scheme"),
    ("1  /  2", "More / fewer iterations"),
    ("3  /  4", "Finer / coarser cells"),
    ("I", "Show or hide the readout"),
    ("F1", "This sheet"),
    ("F2", "Seahorse Valley"),
    ("F3", "Elephant Valley"),
    ("F4", "Mini Mandelbrot"),
    ("Esc", "Close this sheet"),
];

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color) {
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::ZERO,
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.to_string(),
        font_size: size,
        color,
        font_weight: weight,
        max_width,
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Draw `body` centred in the horizontal span starting at `left`, on the
/// vertical centre `cy`.
///
/// Both offsets are measured rather than guessed: `guitk::text` asks the same
/// font the compositor will shape with. Vertically the anchor is the line box,
/// not the em size, because `y` on a `Text` command is the top of the line and
/// a line is taller than its size.
///
/// The start is clamped to `left` because centring is a subtraction, and a
/// label wider than its box centres to a *negative* offset -- half the overflow
/// hangs off each side. Clamping, and letting the toolkit elide the tail, keeps
/// an over-long label inside the thing it labels. The 120x90 window found this:
/// "Mandelbrot Explorer" is wider than a 108-pixel help panel, and started
/// four pixels to the left of it.
fn centred_in(
    f: &mut Frame,
    left: f32,
    span: f32,
    cy: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    label(
        f,
        text::center_x(body, left + span / 2.0, size, weight).max(left),
        cy - text::line_height(size, weight) / 2.0,
        body,
        size,
        color,
        weight,
        Some(span.max(0.0)),
    );
}

// ── Wiring ──────────────────────────────────────────────────────────

/// One body for both the window and the probe, so a test exercises the code
/// the compositor runs rather than a parallel copy of it.
fn handle_event(app: &mut MandelbrotApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ke) => app.handle_key(ke),
        Event::Mouse(me) => app.handle_mouse(me),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for MandelbrotApp {
    fn title(&self) -> String {
        "Mandelbrot".to_string()
    }

    fn app_id(&self) -> String {
        "mandelbrot".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Belt and braces: a compositor that draws before it resizes would
        // otherwise leave the click mapping one frame behind the window.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for MandelbrotApp {
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
    let mut app = MandelbrotApp::new();
    app::launch("mandelbrot", &mut app)
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::probe;

    const SIZE: (f32, f32) = MandelbrotApp::SIZE;

    /// A window's worth of coarse cells.
    ///
    /// Cell size is what a test pays for: 16px cells over the probe window is
    /// about 1,900 sample points, against 30,000 at the shipped default, and
    /// the samples are the entire cost of a frame. 16 rather than something
    /// larger because it is the coarsest the resolution keys can reach, and a
    /// fixture outside the reachable range is a fixture testing a state the
    /// program cannot be in.
    fn coarse() -> MandelbrotApp {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 16.0;
        app
    }

    fn windowed(width: f32, height: f32) -> MandelbrotApp {
        let mut app = coarse();
        app.resize(width, height);
        app
    }

    fn release(key: Key) -> KeyEvent {
        let mut ke = probe::press(key);
        ke.pressed = false;
        ke
    }

    fn press(app: &mut MandelbrotApp, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn scroll(app: &mut MandelbrotApp, x: f32, y: f32, dy: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }),
        )
    }

    fn click(app: &mut MandelbrotApp, x: f32, y: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    /// The sizes every layout invariant is checked at: the default, a shape
    /// that is nothing like it, and two that are barely a window at all.
    const SIZES: [(f32, f32); 9] = [
        (800.0, 600.0),
        (1920.0, 1080.0),
        (640.0, 480.0),
        (400.0, 900.0),
        (900.0, 400.0),
        (240.0, 180.0),
        (120.0, 90.0),
        (60.0, 40.0),
        (20.0, 20.0),
    ];

    // ── Escape iteration ────────────────────────────────────────────

    #[test]
    fn the_origin_never_escapes() {
        assert_eq!(mandelbrot_iter(0.0, 0.0, 100), 100);
    }

    #[test]
    fn the_period_two_point_never_escapes() {
        assert_eq!(mandelbrot_iter(-1.0, 0.0, 100), 100);
    }

    #[test]
    fn the_cardioid_centre_never_escapes() {
        assert_eq!(mandelbrot_iter(-0.25, 0.0, 100), 100);
    }

    #[test]
    fn a_point_well_outside_escapes_at_once() {
        assert!(mandelbrot_iter(2.0, 2.0, 100) < 5);
    }

    #[test]
    fn a_point_on_the_boundary_takes_its_time() {
        assert!(mandelbrot_iter(-0.75, 0.01, 1000) > 10);
    }

    #[test]
    fn the_first_iterate_is_the_earliest_escape_there_is() {
        // z0 = 0 is always inside the escape radius, so no point can be seen to
        // escape before one step -- however far out it is.
        assert_eq!(mandelbrot_iter(10.0, 10.0, 100), 1);
    }

    #[test]
    fn the_set_is_symmetric_about_the_real_axis() {
        assert_eq!(
            mandelbrot_iter(-0.5, 0.5, 200),
            mandelbrot_iter(-0.5, -0.5, 200)
        );
    }

    // ── Colour ──────────────────────────────────────────────────────

    #[test]
    fn every_scheme_paints_the_interior_black() {
        for scheme in ColorScheme::ALL {
            assert_eq!(scheme.color(100, 100), Color::from_hex(0x000000));
        }
    }

    #[test]
    fn no_scheme_paints_the_exterior_black() {
        for scheme in ColorScheme::ALL {
            assert_ne!(scheme.color(50, 100), Color::from_hex(0x000000));
        }
    }

    #[test]
    fn the_schemes_form_one_cycle() {
        let mut cs = ColorScheme::Classic;
        for _ in 0..ColorScheme::ALL.len() {
            cs = cs.next();
        }
        assert_eq!(cs, ColorScheme::Classic);
        for cs in ColorScheme::ALL {
            assert!(!cs.name().is_empty());
        }
    }

    #[test]
    fn a_zero_iteration_budget_is_all_interior_not_a_nan() {
        // The colourist divides by `max_iter`. Nothing in the UI can make it
        // zero, and the interior test returns before the divide -- but a divide
        // that is safe only because of a distant invariant is a divide waiting
        // for that invariant to move, so both guards are stated here.
        for scheme in ColorScheme::ALL {
            let c = scheme.color(0, 0);
            assert_eq!(c, Color::from_hex(0x000000));
        }
    }

    #[test]
    fn the_primaries_survive_the_colour_wheel() {
        assert_eq!(hsv_to_color(0.0, 1.0, 1.0), Color::rgb(255, 0, 0));
        assert_eq!(hsv_to_color(120.0, 1.0, 1.0), Color::rgb(0, 255, 0));
        assert_eq!(hsv_to_color(240.0, 1.0, 1.0), Color::rgb(0, 0, 255));
        assert_eq!(hsv_to_color(0.0, 0.0, 1.0), Color::rgb(255, 255, 255));
        assert_eq!(hsv_to_color(0.0, 0.0, 0.0), Color::rgb(0, 0, 0));
    }

    // ── View ────────────────────────────────────────────────────────

    #[test]
    fn a_fresh_explorer_is_looking_at_the_whole_set() {
        let app = MandelbrotApp::new();
        assert_eq!(app.center_x, -0.5);
        assert_eq!(app.center_y, 0.0);
        assert_eq!(app.max_iter, 100);
        assert!(app.scale > 3.0);
    }

    #[test]
    fn reset_puts_back_everything_the_exploring_changed() {
        let mut app = coarse();
        app.center_x = 1.0;
        app.center_y = 1.0;
        app.scale = 0.01;
        app.max_iter = 900;
        app.reset_view();
        assert_eq!(app.center_x, -0.5);
        assert_eq!(app.center_y, 0.0);
        assert_eq!(app.scale, 3.5);
        assert_eq!(app.max_iter, 100);
    }

    #[test]
    fn zooming_in_narrows_the_view_and_out_widens_it() {
        let mut app = coarse();
        let start = app.scale;
        app.zoom_in();
        assert!(app.scale < start);
        app.zoom_out();
        assert!((app.scale - start).abs() < 1e-12);
    }

    #[test]
    fn the_zoom_stops_where_the_arithmetic_does() {
        // Past MIN_SCALE neighbouring cells map to the same `f64`, so the
        // picture stops being detail and starts being noise. Refusing is the
        // honest answer; carrying on and showing mush is not.
        let mut app = coarse();
        for _ in 0..500 {
            app.zoom_in();
        }
        assert!(app.scale >= MIN_SCALE, "zoomed past the mantissa");
        assert!(!app.enabled(Action::ZoomIn));

        for _ in 0..500 {
            app.zoom_out();
        }
        assert!(app.scale <= MAX_SCALE);
        assert!(!app.enabled(Action::ZoomOut));
    }

    #[test]
    fn panning_moves_the_view_the_way_the_key_points() {
        let mut app = coarse();
        let (x0, y0) = (app.center_x, app.center_y);
        app.pan(1.0, 0.0);
        assert!(app.center_x > x0);
        app.pan(0.0, 1.0);
        assert!(app.center_y > y0);
    }

    #[test]
    fn a_pan_is_a_fraction_of_what_is_on_screen_not_a_fixed_distance() {
        // Otherwise one arrow press crosses the whole view when zoomed in and
        // does nothing visible when zoomed out.
        let mut wide = coarse();
        let mut deep = coarse();
        deep.scale = wide.scale / 100.0;
        let (wx, dx) = (wide.center_x, deep.center_x);
        wide.pan(1.0, 0.0);
        deep.pan(1.0, 0.0);
        assert!((wide.center_x - wx) > (deep.center_x - dx) * 50.0);
    }

    #[test]
    fn the_iteration_count_has_a_floor_and_a_ceiling() {
        let mut app = coarse();
        app.increase_iterations();
        assert_eq!(app.max_iter, 150);
        app.decrease_iterations();
        assert_eq!(app.max_iter, 100);

        app.max_iter = 2000;
        app.increase_iterations();
        assert_eq!(app.max_iter, 2000);
        app.max_iter = 25;
        app.decrease_iterations();
        assert_eq!(app.max_iter, 25);
    }

    #[test]
    fn the_cell_size_has_a_floor_and_a_ceiling() {
        let mut app = coarse();
        app.pixel_size = 4.0;
        app.increase_resolution();
        assert_eq!(app.pixel_size, 3.0);
        app.decrease_resolution();
        assert_eq!(app.pixel_size, 4.0);

        app.pixel_size = 1.0;
        app.increase_resolution();
        assert_eq!(app.pixel_size, 1.0);
        app.pixel_size = 16.0;
        app.decrease_resolution();
        assert_eq!(app.pixel_size, 16.0);
    }

    // ── Coordinates ─────────────────────────────────────────────────

    #[test]
    fn the_middle_of_the_window_is_the_centre_of_the_view() {
        for (w, h) in SIZES {
            let app = windowed(w, h);
            let (cx, cy) = app.complex_at(w / 2.0, h / 2.0);
            assert!((cx - app.center_x).abs() < 1e-9, "{w}x{h}");
            assert!((cy - app.center_y).abs() < 1e-9, "{w}x{h}");
        }
    }

    #[test]
    fn a_click_is_read_against_the_window_it_was_drawn_in() {
        // The old handler mapped every click through a hard-coded 800x600, so
        // the further the real window was from that, the further the click
        // landed from where it was aimed. Two different windows, the same
        // fraction across each: the same complex point.
        let a = windowed(800.0, 600.0);
        let b = windowed(1600.0, 1200.0);
        let (ax, ay) = a.complex_at(800.0 * 0.25, 600.0 * 0.75);
        let (bx, by) = b.complex_at(1600.0 * 0.25, 1200.0 * 0.75);
        assert!((ax - bx).abs() < 1e-12, "{ax} vs {bx}");
        assert!((ay - by).abs() < 1e-12, "{ay} vs {by}");
    }

    #[test]
    fn a_wide_window_shows_more_of_the_plane_sideways_not_a_stretched_set() {
        // The aspect correction goes on the horizontal span; if it were dropped
        // the set would be drawn as an ellipse.
        let app = windowed(1000.0, 500.0);
        let (left, _) = app.complex_at(0.0, 250.0);
        let (right, _) = app.complex_at(1000.0, 250.0);
        let (_, top) = app.complex_at(500.0, 0.0);
        let (_, bottom) = app.complex_at(500.0, 500.0);
        let span_x = right - left;
        let span_y = bottom - top;
        assert!((span_x / span_y - 2.0).abs() < 1e-9, "{span_x} / {span_y}");
    }

    #[test]
    fn what_is_drawn_at_a_pixel_is_what_a_click_there_selects() {
        // The single fault behind both the stretched image and the missed
        // click: the renderer indexed cells while the click mapped pixels.
        // They are one function now, and this is the statement of that.
        let app = windowed(800.0, 600.0);
        let cell = app.ensure_tile(800.0, 600.0);
        let slot = app.tile.borrow();
        let tile = slot.as_ref().unwrap();
        for (col, row) in [(0usize, 0usize), (5, 4), (11, 9)] {
            let sx = col as f32 * cell + cell / 2.0;
            let sy = row as f32 * cell + cell / 2.0;
            let (cx, cy) = app.complex_at(sx, sy);
            assert_eq!(
                tile.iters[row * tile.cols + col],
                mandelbrot_iter(cx, cy, app.max_iter),
                "cell ({col},{row})"
            );
        }
    }

    #[test]
    fn the_whole_window_is_painted() {
        // `cols = (width / ps) as usize` truncated, so the rightmost partial
        // column and the bottom partial row were left as bare background.
        for (w, h) in SIZES {
            let app = windowed(w, h);
            let cell = app.effective_cell(w, h);
            let f = app.frame(w, h);
            let mut right = 0.0_f32;
            let mut bottom = 0.0_f32;
            for cmd in f.commands() {
                if let RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } = cmd
                {
                    if (*width - cell).abs() < 0.001 && (*height - cell).abs() < 0.001 {
                        right = right.max(x + width);
                        bottom = bottom.max(y + height);
                    }
                }
            }
            assert!(right >= w, "{w}x{h}: painted to {right}, window is {w}");
            assert!(bottom >= h, "{w}x{h}: painted to {bottom}, window is {h}");
        }
    }

    // ── The frame budget ────────────────────────────────────────────

    #[test]
    fn a_frame_costs_a_bounded_amount_of_arithmetic() {
        // A maximised window at one pixel per cell and the 2000-iteration cap
        // is four billion escape iterations for one frame, on the thread that
        // answers the keyboard.
        let mut app = MandelbrotApp::new();
        app.pixel_size = 1.0;
        app.max_iter = 2000;
        for (w, h) in [(1920.0_f32, 1080.0_f32), (3840.0, 2160.0), (800.0, 600.0)] {
            let cell = app.effective_cell(w, h);
            let cols = (w / cell).ceil() as u64;
            let rows = (h / cell).ceil() as u64;
            let work = cols * rows * u64::from(app.max_iter);
            assert!(
                work <= FRAME_ITER_BUDGET || cell >= MAX_CELL,
                "{w}x{h}: {work} iterations at cell {cell}"
            );
        }
    }

    #[test]
    fn the_requested_resolution_is_honoured_when_it_fits() {
        // The budget is a ceiling, not a policy: an ordinary window at an
        // ordinary setting must get exactly what it asked for.
        let mut app = MandelbrotApp::new();
        app.pixel_size = 8.0;
        app.max_iter = 100;
        assert_eq!(app.effective_cell(800.0, 600.0), 8.0);
        assert!(!app.resolution_capped());
    }

    #[test]
    fn the_readout_says_when_the_resolution_was_overridden() {
        // Silently ignoring a setting is worse than refusing it: the user
        // presses 3 four times and cannot tell whether it did anything.
        let mut app = MandelbrotApp::new();
        app.pixel_size = 1.0;
        app.max_iter = 2000;
        app.resize(1920.0, 1080.0);
        assert!(app.resolution_capped());
        assert!(app.info_text().contains("drawn at"));

        app.pixel_size = 8.0;
        app.max_iter = 100;
        assert!(!app.resolution_capped());
        assert!(!app.info_text().contains("drawn at"));
    }

    // ── The cache ───────────────────────────────────────────────────

    #[test]
    fn drawing_the_same_view_twice_computes_it_once() {
        let app = windowed(800.0, 600.0);
        app.ensure_tile(800.0, 600.0);
        let first = app.computes.get();
        assert_eq!(first, 1);
        for _ in 0..5 {
            let _ = app.frame(800.0, 600.0);
        }
        assert_eq!(app.computes.get(), 1, "an unchanged view was recomputed");
    }

    #[test]
    fn recolouring_does_not_recompute_the_fractal() {
        // The grid holds escape counts, not colours, which is the whole reason
        // the colour scheme is not part of the cache key -- and the difference
        // between `C` being instant and `C` costing a full frame's arithmetic.
        let mut app = windowed(800.0, 600.0);
        let _ = app.frame(800.0, 600.0);
        let before = app.computes.get();
        for _ in 0..ColorScheme::ALL.len() {
            press(&mut app, Key::C);
            let _ = app.frame(800.0, 600.0);
        }
        assert_eq!(app.computes.get(), before);
    }

    #[test]
    fn moving_the_view_does_recompute_it() {
        let mut app = windowed(800.0, 600.0);
        let _ = app.frame(800.0, 600.0);
        let before = app.computes.get();
        press(&mut app, Key::Right);
        let _ = app.frame(800.0, 600.0);
        assert_eq!(app.computes.get(), before + 1);
    }

    #[test]
    fn resizing_the_window_recomputes_the_fractal() {
        let app = windowed(800.0, 600.0);
        let _ = app.frame(800.0, 600.0);
        let _ = app.frame(640.0, 480.0);
        assert_eq!(app.computes.get(), 2);
    }

    // ── Keys ────────────────────────────────────────────────────────

    #[test]
    fn a_release_is_not_a_second_press() {
        // Every action ran twice, once on each edge. For `Z` that was a double
        // zoom; for the two toggles it was no visible effect at all.
        let mut app = coarse();
        press(&mut app, Key::Z);
        let once = app.scale;
        assert_eq!(
            handle_event(&mut app, &Event::Key(release(Key::Z))),
            EventResult::Ignored
        );
        assert_eq!(app.scale, once, "the release zoomed again");
    }

    #[test]
    fn the_readout_toggle_survives_the_key_being_let_go() {
        let mut app = coarse();
        assert!(app.show_info);
        press(&mut app, Key::I);
        handle_event(&mut app, &Event::Key(release(Key::I)));
        assert!(!app.show_info, "the release toggled it back");
    }

    #[test]
    fn the_help_sheet_can_actually_be_opened() {
        // It could not before: F1 set the flag on press and cleared it on
        // release, so the sheet existed for no frame at all.
        let mut app = coarse();
        assert!(!app.show_help);
        press(&mut app, Key::F1);
        handle_event(&mut app, &Event::Key(release(Key::F1)));
        assert!(app.show_help);
    }

    #[test]
    fn the_help_sheet_closes_on_the_keys_that_close_things() {
        for key in [Key::F1, Key::Escape, Key::Enter, Key::Space] {
            let mut app = coarse();
            app.show_help = true;
            press(&mut app, key);
            assert!(!app.show_help, "{key:?} did not close it");
        }
    }

    #[test]
    fn keys_do_nothing_behind_the_help_sheet() {
        // Panning a view you cannot see is not a feature; it is a way to lose
        // your place while reading the legend.
        let mut app = coarse();
        app.show_help = true;
        let (x, y, s) = (app.center_x, app.center_y, app.scale);
        for key in [Key::Right, Key::Up, Key::Z, Key::X, Key::R] {
            press(&mut app, key);
        }
        assert_eq!((app.center_x, app.center_y, app.scale), (x, y, s));
    }

    #[test]
    fn a_ctrl_or_alt_combination_belongs_to_the_desktop() {
        // Ctrl-R is the desktop's reload, not the explorer's reset.
        let mut app = coarse();
        let before = (app.center_x, app.scale, app.color_scheme);
        for build in [
            (|k| probe::ctrl(k)) as fn(Key) -> KeyEvent,
            |k| {
                let mut ke = probe::press(k);
                ke.modifiers.alt = true;
                ke
            },
            |k| {
                let mut ke = probe::press(k);
                ke.modifiers.super_key = true;
                ke
            },
        ] {
            for key in [Key::R, Key::Z, Key::C] {
                assert_eq!(
                    handle_event(&mut app, &Event::Key(build(key))),
                    EventResult::Ignored,
                    "{key:?} with a modifier was acted on"
                );
            }
        }
        assert_eq!((app.center_x, app.scale, app.color_scheme), before);
    }

    #[test]
    fn the_arrow_keys_pan_the_way_they_point() {
        let mut app = coarse();
        let (x, y) = (app.center_x, app.center_y);
        press(&mut app, Key::Up);
        assert!(app.center_y < y);
        press(&mut app, Key::Down);
        press(&mut app, Key::Right);
        assert!(app.center_x > x);
        press(&mut app, Key::Left);
        assert!((app.center_x - x).abs() < 1e-12);
    }

    #[test]
    fn both_spellings_of_zoom_do_the_same_thing() {
        // The module documentation always promised `+`/`-`; only `Z`/`X` were
        // wired.
        for (into, out) in [(Key::Z, Key::X), (Key::Equals, Key::Minus)] {
            let mut app = coarse();
            let start = app.scale;
            press(&mut app, into);
            assert!(app.scale < start, "{into:?} did not zoom in");
            press(&mut app, out);
            assert!((app.scale - start).abs() < 1e-12, "{out:?} did not undo it");
        }
    }

    #[test]
    fn the_presets_go_where_they_say() {
        for (key, x) in [(Key::F2, -0.745), (Key::F3, 0.281_717), (Key::F4, -1.7497)] {
            let mut app = coarse();
            press(&mut app, key);
            assert!((app.center_x - x).abs() < 1e-6, "{key:?}");
            assert!(app.scale < 0.1, "{key:?} did not zoom in");
            assert!(app.max_iter > 100, "{key:?} did not raise the detail");
        }
    }

    #[test]
    fn the_number_keys_move_the_two_dials() {
        let mut app = coarse();
        press(&mut app, Key::Num1);
        assert_eq!(app.max_iter, 150);
        press(&mut app, Key::Num2);
        assert_eq!(app.max_iter, 100);
        let ps = app.pixel_size;
        press(&mut app, Key::Num3);
        assert!(app.pixel_size < ps);
        press(&mut app, Key::Num4);
        assert_eq!(app.pixel_size, ps);
    }

    #[test]
    fn a_key_the_explorer_has_no_use_for_is_left_alone() {
        let mut app = coarse();
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::press(Key::Q))),
            EventResult::Ignored
        );
    }

    // ── Pointer ─────────────────────────────────────────────────────

    #[test]
    fn clicking_the_fractal_centres_on_what_was_clicked_and_closes_in() {
        let mut app = windowed(640.0, 480.0);
        let (wanted_x, wanted_y) = app.complex_at(100.0, 380.0);
        let before = app.scale;
        assert_eq!(click(&mut app, 100.0, 380.0), EventResult::Consumed);
        assert!((app.center_x - wanted_x).abs() < 1e-12);
        assert!((app.center_y - wanted_y).abs() < 1e-12);
        assert!(app.scale < before);
    }

    #[test]
    fn the_wheel_zooms_about_the_pointer() {
        // The point under the cursor is the one being examined; zooming about
        // the centre instead walks it off the screen.
        let mut app = windowed(800.0, 600.0);
        let (px, py) = (620.0_f32, 140.0_f32);
        let (before_x, before_y) = app.complex_at(px, py);
        scroll(&mut app, px, py, 1.0);
        let (after_x, after_y) = app.complex_at(px, py);
        assert!(app.scale < 3.5, "the wheel did not zoom in");
        assert!(
            (after_x - before_x).abs() < 1e-12,
            "{before_x} -> {after_x}"
        );
        assert!(
            (after_y - before_y).abs() < 1e-12,
            "{before_y} -> {after_y}"
        );
    }

    #[test]
    fn the_wheel_turns_both_ways_and_stops_for_nothing() {
        let mut app = windowed(800.0, 600.0);
        let start = app.scale;
        scroll(&mut app, 400.0, 300.0, -1.0);
        assert!(app.scale > start);
        scroll(&mut app, 400.0, 300.0, 1.0);
        assert!((app.scale - start).abs() < 1e-12);
        assert_eq!(scroll(&mut app, 400.0, 300.0, 0.0), EventResult::Ignored);
    }

    // ── Buttons ─────────────────────────────────────────────────────

    #[test]
    fn every_button_is_the_action_it_names() {
        for (index, (action, _)) in BUTTONS.iter().enumerate() {
            let l = Layout::new(SIZE.0, SIZE.1);
            let r = l.button(index);
            let (cx, cy) = r.centre();

            let mut clicked = windowed(SIZE.0, SIZE.1);
            assert_eq!(
                clicked.target_at(cx, cy),
                Some(Target::Button(*action)),
                "button {index} does not hit-test as {action:?}"
            );
            click(&mut clicked, cx, cy);

            let mut applied = windowed(SIZE.0, SIZE.1);
            applied.apply(*action);

            assert_eq!(clicked.scale, applied.scale, "{action:?} scale");
            assert_eq!(clicked.center_x, applied.center_x, "{action:?} centre");
            assert_eq!(
                clicked.color_scheme, applied.color_scheme,
                "{action:?} scheme"
            );
            assert_eq!(clicked.show_help, applied.show_help, "{action:?} help");
        }
    }

    #[test]
    fn the_buttons_offer_the_zooming_the_wheel_does() {
        // Someone with no wheel, or no keyboard, still has to be able to move.
        let named: Vec<Action> = BUTTONS.iter().map(|(a, _)| *a).collect();
        for wanted in [
            Action::ZoomIn,
            Action::ZoomOut,
            Action::Reset,
            Action::CycleScheme,
            Action::ToggleHelp,
        ] {
            assert!(named.contains(&wanted), "no button for {wanted:?}");
        }
    }

    #[test]
    fn a_button_that_can_do_nothing_still_takes_its_own_click() {
        // Otherwise the click falls through to the fractal underneath and
        // recentres the view on the button the user was aiming at.
        let mut app = windowed(SIZE.0, SIZE.1);
        app.scale = MAX_SCALE;
        assert!(!app.enabled(Action::ZoomOut));
        let index = BUTTONS
            .iter()
            .position(|(a, _)| *a == Action::ZoomOut)
            .unwrap();
        let (cx, cy) = Layout::new(SIZE.0, SIZE.1).button(index).centre();
        let centre = app.center_x;
        assert_eq!(click(&mut app, cx, cy), EventResult::Consumed);
        assert_eq!(app.center_x, centre, "the dim button leaked its click");
        assert_eq!(app.scale, MAX_SCALE);
    }

    #[test]
    fn the_buttons_follow_the_window_when_it_is_resized() {
        let mut app = windowed(SIZE.0, SIZE.1);
        let wide = Layout::new(1400.0, 900.0).button(0);
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1400,
                height: 900,
            },
        );
        let (cx, cy) = wide.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Button(BUTTONS[0].0)));
    }

    #[test]
    fn a_window_too_narrow_for_buttons_drops_them_rather_than_the_fractal() {
        let app = windowed(120.0, 90.0);
        let l = Layout::new(120.0, 90.0);
        assert!(!l.shows_buttons());
        let f = app.frame(120.0, 90.0);
        assert!(
            f.hits()
                .iter()
                .all(|(t, _)| !matches!(t, Target::Button(_))),
            "buttons were recorded in a window with no room for them"
        );
        assert_eq!(app.target_at(60.0, 45.0), Some(Target::Fractal));
    }

    // ── The help sheet ──────────────────────────────────────────────

    #[test]
    fn the_help_sheet_is_in_front_of_the_buttons_it_covers() {
        // `hit_test` searches backwards, so the sheet has to be drawn last or
        // a click meant to dismiss it presses a button through it.
        let mut app = windowed(SIZE.0, SIZE.1);
        let (cx, cy) = Layout::new(SIZE.0, SIZE.1).button(0).centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Button(BUTTONS[0].0)));
        app.show_help = true;
        assert_eq!(app.target_at(cx, cy), Some(Target::Help));
    }

    #[test]
    fn a_click_anywhere_dismisses_the_help_sheet() {
        for (x, y) in [(400.0, 300.0), (5.0, 5.0), (795.0, 595.0)] {
            let mut app = windowed(SIZE.0, SIZE.1);
            app.show_help = true;
            let before = app.center_x;
            assert_eq!(click(&mut app, x, y), EventResult::Consumed);
            assert!(!app.show_help, "({x},{y}) did not dismiss it");
            assert_eq!(app.center_x, before, "the click reached the fractal");
        }
    }

    #[test]
    fn the_help_sheet_stays_inside_the_window_it_is_shown_in() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            assert!(l.help.x >= -0.001 && l.help.y >= -0.001, "{w}x{h}");
            assert!(l.help.right() <= w + 0.001, "{w}x{h}");
            assert!(l.help.bottom() <= h + 0.001, "{w}x{h}");
        }
    }

    #[test]
    fn the_help_sheet_never_writes_past_its_own_panel() {
        // A legend that runs off the bottom of its box is worse than a short
        // one, because the reader cannot tell that it did.
        for (w, h) in SIZES {
            let mut app = windowed(w, h);
            app.show_help = true;
            let l = Layout::new(w, h);
            let f = app.frame(w, h);
            for cmd in f.commands() {
                if let RenderCommand::Text {
                    x,
                    y,
                    text: body,
                    font_size,
                    font_weight,
                    ..
                } = cmd
                {
                    // The readout and the buttons live outside the panel by
                    // design, so the sheet's own text is picked out by what it
                    // says rather than by where it is -- which is the thing
                    // under test and so cannot also be the filter.
                    let mine = body == "Mandelbrot Explorer"
                        || HELP_ROWS.iter().any(|(k, d)| body == k || body == d);
                    if !mine {
                        continue;
                    }
                    let bottom = y + text::line_height(*font_size, *font_weight);
                    assert!(
                        bottom <= l.help.bottom() + 0.001 && *x >= l.help.x - 0.001,
                        "{w}x{h}: {body:?} at ({x}, {y}) ends at {bottom}, panel is {:?}",
                        l.help
                    );
                }
            }
        }
    }

    // ── Layout and frames ───────────────────────────────────────────

    #[test]
    fn the_layout_stays_inside_the_window_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("info", l.info),
                ("footer", l.footer),
                ("fractal", l.fractal),
                ("help", l.help),
            ] {
                assert!(r.x >= -0.001 && r.y >= -0.001, "{name} at {w}x{h}");
                assert!(r.right() <= w + 0.001, "{name} at {w}x{h}");
                assert!(r.bottom() <= h + 0.001, "{name} at {w}x{h}");
            }
            assert!(l.info.bottom() <= l.footer.y + 0.001, "bars overlap");
        }
    }

    #[test]
    fn the_bars_never_cover_the_whole_window() {
        // Two bars that meet in the middle leave no fractal, which is the one
        // thing this program is for.
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            assert!(
                l.info.h + l.footer.h <= h + 0.001,
                "{w}x{h}: bars are {} of {h}",
                l.info.h + l.footer.h
            );
        }
    }

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        for (w, h) in SIZES {
            for (info, help) in [(true, false), (false, false), (true, true)] {
                let mut app = windowed(w, h);
                app.show_info = info;
                app.show_help = help;
                let f = app.frame(w, h);
                assert!(f.is_balanced(), "{w}x{h} info={info} help={help}");
                assert!(!f.commands().is_empty(), "{w}x{h} drew nothing");
            }
        }
    }

    #[test]
    fn hiding_the_readout_hides_the_readout() {
        let mut app = windowed(SIZE.0, SIZE.1);
        let shown = app.frame(SIZE.0, SIZE.1);
        let has_centre = |f: &Frame| {
            f.commands().iter().any(
                |c| matches!(c, RenderCommand::Text { text, .. } if text.starts_with("Center:")),
            )
        };
        assert!(has_centre(&shown));
        app.show_info = false;
        assert!(!has_centre(&app.frame(SIZE.0, SIZE.1)));
    }

    #[test]
    fn a_resize_event_is_what_the_next_frame_is_drawn_at() {
        let mut app = coarse();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1024,
                height: 768,
            },
        );
        assert_eq!((app.width, app.height), (1024.0, 768.0));
        let f = app.frame(app.width, app.height);
        assert_eq!((f.width, f.height), (1024.0, 768.0));
    }

    #[test]
    fn drawing_at_a_new_size_is_what_a_click_is_then_read_against() {
        // `render` takes the size as an argument; a compositor that draws
        // before it resizes would otherwise leave the click mapping a frame
        // behind the window.
        let mut app = coarse();
        let _ = app.render(1024.0, 768.0);
        assert_eq!((app.width, app.height), (1024.0, 768.0));
    }

    #[test]
    fn the_window_says_what_it_is() {
        let app = MandelbrotApp::new();
        assert_eq!(app.title(), "Mandelbrot");
        assert_eq!(app.app_id(), "mandelbrot");
        assert_eq!(app.initial_size(), (800, 600));
    }

    #[test]
    fn an_event_that_changes_nothing_does_not_cost_a_repaint() {
        let mut app = coarse();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::R))),
            Response::Redraw
        );
    }
}
