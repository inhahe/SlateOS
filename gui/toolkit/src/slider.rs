//! A value chosen by dragging a thumb along a track: the toolkit's slider.
//!
//! # Why the toolkit has one now
//!
//! `WidgetKind::Slider` was declared when the widget tree was written, and
//! nothing ever drew it or answered a click on it. Everything that needed a
//! slider made its own. The desktop's quick settings, its volume and brightness
//! display and its touchpad page drew theirs through a module in `gui/desktop`
//! -- drawing only: the quick settings' two could be clicked but not dragged,
//! and only on the track's exact pixels, so a thumb sitting at either end
//! hung half outside the part that answered. A dozen applications drew theirs
//! by hand. A slider is a control a user learns once, and a dozen copies teach
//! a dozen different ones.
//!
//! So this is the one: the drawing (moved here from the desktop, with the
//! reasoning that settled it -- see *The thumb is `text`* below), the
//! geometry, and the input -- pressing, dragging, letting go, the keyboard and
//! the wheel.
//!
//! # The region you can take hold of is larger than what is drawn
//!
//! `roadmap-detailed.md` asks, for sliders by name, that "the grab region for
//! a slider handle (and the click-to-jump region along its track) extends past
//! the drawn thumb/track so the user can grab and drag it without
//! pixel-hunting". [`Placement::hit`] is the whole slider's region and
//! [`Placement::thumb_hit`] the thumb's, both by [`crate::grab`]'s rule for a
//! handle: at least 24 pixels across however thin the track, and a margin past
//! the thumb's rim however large it is drawn.
//!
//! A press inside the thumb's region takes hold of the thumb *where it was
//! pressed*, and the thumb keeps that offset from the pointer while dragged:
//! pressing near its rim does not make it jump to centre itself under the
//! pointer. A press elsewhere on the track moves the thumb there and takes
//! hold of it centred -- the click-to-jump the roadmap names.
//!
//! # What it reports: moving, settled, or put back
//!
//! The roadmap asks attribute-choosing controls for "distinct signals for
//! tentative change (may fire many times, freely revertible) vs. committed
//! vs. cancelled (revert), so apps can preview without persisting", and the
//! colour picker already answers that way (`ColorPickerEvent`). A slider says
//! the same three things with the same names:
//!
//! | Event | When | What a host does |
//! |---|---|---|
//! | [`SliderEvent::Changed`] | the value moved during a drag | show it -- play the volume, dim the screen -- but do not save it |
//! | [`SliderEvent::Confirmed`] | a drag was let go, or a key or the wheel moved the value | save it |
//! | [`SliderEvent::Cancelled`] | Escape during a drag | show the value carried, which is where the drag began |
//!
//! A drag that ends where it began confirms nothing, so a host that saves on
//! `Confirmed` does not rewrite a file for a click that changed nothing.
//!
//! # The keyboard, as every other desktop's slider has it
//!
//! Right and Up raise the value by a step, Left and Down lower it, Page Up and
//! Page Down by a page, Home and End go to the ends (the WAI-ARIA slider
//! pattern, which is what the other toolkits implement). Keys with Ctrl, Alt or
//! Super held are the program's shortcuts and are left alone.
//!
//! # The wheel only while the slider has the keyboard
//!
//! [`Slider::wheel`] exists, and a host should send it the wheel only while the
//! slider has the keyboard focus. A page of settings scrolled with the wheel
//! passes its sliders under a resting pointer, and a slider that took the wheel
//! whenever the pointer was over it would change every value the page scrolled
//! past. [`Slider::handle_mouse`] leaves the wheel alone for that reason.
//!
//! # The thumb is `text`, which is the *opposite* rule from a switch knob
//!
//! The switch ([`crate::switch`]) derives its knob with [`readable_on`] of the
//! track. Doing that here would be wrong, and the reason is geometry, not
//! taste.
//!
//! A switch knob is *contained* by its track: inset two pixels from every edge,
//! so the track is the only thing behind it and the only thing it has to be
//! legible against. A slider thumb is **larger than its track** -- 10 to 14
//! pixels of circle on a track 4 or 6 pixels tall -- so most of it hangs over
//! the panel. Its silhouette, the round outline that says *this is the handle*,
//! is drawn against the card, not against the fill. Pick the ink for the fill
//! and you lose the silhouette: on the stock dark theme with a blue accent, a
//! thumb of `readable_on(accent)` is `crust`, which on a `base` card is 1.1:1 --
//! an invisible handle with a crisp interior nobody can see.
//!
//! `text` is the other way round: 11.34:1 against the card and 1.46:1 against
//! the fill. The weak number is the one that costs nothing, because the fill
//! only ever touches the thumb's *interior*, which carries no information once
//! you can see the circle. (Before these sliders were one module, one of the
//! desktop's copies drew its thumb in the accent -- the fill's own colour,
//! 1.00:1 -- and for the left half of its travel the handle was simply not
//! there.)
//!
//! # The geometry was recovered, not invented
//!
//! The desktop's five hand-drawn sliders all agreed on it before they became
//! one: the track's corner radius is half its thickness, and the thumb is a
//! circle of its own diameter centred on the end of the filled part, on the
//! track's midline -- so at either end of the travel it overhangs the track by
//! half its width. That held at every size the desktop draws (tracks 4 and 6
//! pixels thick, thumbs 10, 12 and 14), and it is what [`draw_bar`] draws.

use crate::color::Color;
use crate::disabled::{DISABLED_OPACITY, DisabledState};
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::grab;
use crate::palette::Palette;
#[cfg(doc)]
use crate::palette::readable_on;
use crate::render::RenderCommand;
use crate::style::CornerRadii;
use crate::surface::CommandSink;

/// How far the light round a thumb reaches past its rim while the pointer is
/// on it or it is held.
///
/// The light is how a user learns that the pointer, sitting in the generous
/// region beside the thumb rather than on its pixels, will take hold of it --
/// without it, the enlarged region is a rule nobody can see.
pub const HALO: f32 = 4.0;

/// How opaque that light is, before the slider's own alpha: a quarter.
const HALO_ALPHA: u8 = 64;

/// The space between the thumb's rim and the keyboard focus ring round it.
const FOCUS_GAP: f32 = 2.0;

/// Of a slider with no step of its own, how much of its range one arrow key
/// moves it: a hundredth.
const CONTINUOUS_LINE_DIVISIONS: f64 = 100.0;

/// And how much one Page Up moves it, with or without a step: a tenth, or one
/// step if a step is larger.
const PAGE_DIVISIONS: f64 = 10.0;

/// A grid index this close to a whole number is on it. The grid is `min + k *
/// step` in `f64`, and a value set to a grid point can read back a hair either
/// side of it (`0.1 * 3` is `0.30000000000000004`), which would otherwise make
/// an arrow key step to the point it is already on.
const GRID_EPSILON: f64 = 1e-9;

// ============================================================================
// Geometry
// ============================================================================

/// Which way a slider's track runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Left to right, the low end at the left.
    #[default]
    Horizontal,
    /// Bottom to top, the low end at the bottom -- as a volume control's
    /// level rises.
    Vertical,
}

/// Where a slider is drawn: its track, its thumb's size, and which way it runs.
///
/// A host builds one from its own layout -- the same numbers it draws with --
/// and hands it to both the drawing and the input, so the two cannot disagree
/// about where the slider is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The track as drawn. For a horizontal slider `h` is its thickness; for a
    /// vertical one, `w`. At either end of the travel the thumb overhangs the
    /// track by half its diameter.
    pub track: Rect,
    /// The thumb's diameter.
    pub thumb: f32,
    /// Which way the track runs.
    pub orientation: Orientation,
}

impl Placement {
    /// A horizontal slider on `track`, with a thumb `thumb` across.
    #[must_use]
    pub const fn horizontal(track: Rect, thumb: f32) -> Self {
        Self {
            track,
            thumb,
            orientation: Orientation::Horizontal,
        }
    }

    /// A vertical slider on `track`, low at the bottom.
    #[must_use]
    pub const fn vertical(track: Rect, thumb: f32) -> Self {
        Self {
            track,
            thumb,
            orientation: Orientation::Vertical,
        }
    }

    /// The thumb's diameter, with a negative or non-finite one read as none.
    fn thumb_size(&self) -> f32 {
        if self.thumb.is_finite() {
            self.thumb.max(0.0)
        } else {
            0.0
        }
    }

    /// The centre of the thumb when the slider is `frac` full.
    fn centre_at(&self, frac: f32) -> (f32, f32) {
        let frac = clamp_frac(frac);
        let t = self.track;
        match self.orientation {
            Orientation::Horizontal => (t.x + t.w * frac, t.y + t.h / 2.0),
            Orientation::Vertical => (t.x + t.w / 2.0, t.bottom() - t.h * frac),
        }
    }

    /// Where the thumb is drawn when the slider is `frac` full: a square of
    /// its diameter centred on the end of the fill.
    #[must_use]
    pub fn thumb_rect(&self, frac: f32) -> Rect {
        let (cx, cy) = self.centre_at(frac);
        let d = self.thumb_size();
        Rect::new(cx - d / 2.0, cy - d / 2.0, d, d)
    }

    /// The region a press on the thumb takes hold of it in, when the slider is
    /// `frac` full: the thumb as drawn, grown as [`grab::handle`] grows a
    /// handle.
    #[must_use]
    pub fn thumb_hit(&self, frac: f32) -> Rect {
        grab::handle(self.thumb_rect(frac))
    }

    /// The region a press on this slider lands in at all.
    ///
    /// Everything the thumb can cover -- the track and the half-thumb it
    /// overhangs at each end, as thick as the thumb where the thumb is thicker
    /// -- grown as [`grab::handle`] grows a handle. This is the rectangle to
    /// record as the slider's hit box.
    #[must_use]
    pub fn hit(&self) -> Rect {
        let t = self.track;
        let d = self.thumb_size();
        let covered = match self.orientation {
            Orientation::Horizontal => {
                let across = t.h.max(d);
                Rect::new(
                    t.x - d / 2.0,
                    t.y + t.h / 2.0 - across / 2.0,
                    t.w + d,
                    across,
                )
            }
            Orientation::Vertical => {
                let across = t.w.max(d);
                Rect::new(
                    t.x + t.w / 2.0 - across / 2.0,
                    t.y - d / 2.0,
                    across,
                    t.h + d,
                )
            }
        };
        grab::handle(covered)
    }

    /// The pointer's position along the track's axis.
    const fn along(&self, x: f32, y: f32) -> f32 {
        match self.orientation {
            Orientation::Horizontal => x,
            Orientation::Vertical => y,
        }
    }

    /// How full the slider is with its thumb's centre at `along` on the
    /// track's axis, clamped to the track: a thumb dragged past an end stays
    /// at that end.
    ///
    /// In `f64`, because this becomes the *value*: a pixel position is exact
    /// in `f32`, but `120.0 / 200.0` in `f32` is `0.6000000238`, and a volume
    /// dragged to 60 would be saved as 60.0000024.
    fn frac_at(&self, along: f32) -> f64 {
        let t = self.track;
        let (along, x, w, h) = (
            f64::from(along),
            f64::from(t.x),
            f64::from(t.w),
            f64::from(t.h),
        );
        let raw = match self.orientation {
            Orientation::Horizontal if w > 0.0 => (along - x) / w,
            Orientation::Vertical if h > 0.0 => (f64::from(t.y) + h - along) / h,
            // A track with no length has one position.
            _ => 0.0,
        };
        if raw.is_nan() {
            0.0
        } else {
            raw.clamp(0.0, 1.0)
        }
    }
}

/// `frac` in `0.0..=1.0`, with NaN read as empty.
///
/// `f32::clamp` passes a NaN straight through, and a NaN here would reach the
/// compositor as a rectangle at an undefined position; a fraction that is not
/// a number is not a value the control has.
fn clamp_frac(frac: f32) -> f32 {
    if frac.is_nan() {
        0.0
    } else {
        frac.clamp(0.0, 1.0)
    }
}

// ============================================================================
// Drawing
// ============================================================================

/// How a slider's track is coloured, and how opaque the whole control is.
///
/// The thumb's colour is deliberately **not** here: it is the palette's
/// `text`, for the reason in the module docs, and making it a field is exactly
/// what once produced a thumb the colour of its own fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// The unfilled part of the track -- a surface role.
    pub track: Color,
    /// The filled part. Usually the accent; a slider that means *safe* rather
    /// than *selected* passes green, as the desktop's switch does.
    pub fill: Color,
    /// Opacity applied to every colour, for an overlay that fades as one
    /// thing. `u8::MAX` for an opaque control.
    pub alpha: u8,
}

impl Look {
    /// An opaque slider on `track`, filled with the accent: what most settings
    /// panels draw.
    #[must_use]
    pub const fn accent(p: &Palette, track: Color) -> Self {
        Self {
            track,
            fill: p.accent,
            alpha: u8::MAX,
        }
    }
}

/// Re-apply an alpha to a role colour.
///
/// A role at full opacity is returned as it is rather than re-wrapped, so a
/// palette colour with an alpha of its own survives an opaque control.
fn fade(c: Color, alpha: u8) -> Color {
    if alpha == u8::MAX {
        c
    } else {
        Color::rgba(c.r, c.g, c.b, alpha)
    }
}

/// `alpha` scaled by `factor` in `0.0..=1.0`.
fn scaled_alpha(alpha: u8, factor: f32) -> u8 {
    let scaled = (f32::from(alpha) * factor.clamp(0.0, 1.0)).round();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded, and between 0 and 255 because both factors are"
    )]
    let out = scaled as u8;
    out
}

/// Draw a slider's shapes -- track, fill, thumb, in that order -- for a
/// slider `frac` full.
///
/// The stateless half of [`Slider::draw`], for what is drawn like a slider but
/// is not one: the desktop's volume display, which shows a level and takes no
/// input. A slider at its floor draws no fill: a zero-length rectangle is a
/// command the compositor would carry every frame and could not draw, so
/// callers must not assume a fixed number of commands.
pub fn draw_bar(
    sink: &mut impl CommandSink,
    p: &Palette,
    placement: &Placement,
    frac: f32,
    look: Look,
) {
    paint_bar(sink, placement, frac, look, p.text);
}

/// [`draw_bar`] with the thumb's ink as an argument, which only this module
/// may choose.
fn paint_bar(
    sink: &mut impl CommandSink,
    placement: &Placement,
    frac: f32,
    look: Look,
    thumb: Color,
) {
    let t = placement.track;
    let frac = clamp_frac(frac);
    let thickness = match placement.orientation {
        Orientation::Horizontal => t.h,
        Orientation::Vertical => t.w,
    };
    let radius = CornerRadii::all((thickness / 2.0).max(0.0));
    sink.emit(RenderCommand::FillRect {
        x: t.x,
        y: t.y,
        width: t.w,
        height: t.h,
        color: fade(look.track, look.alpha),
        corner_radii: radius,
    });
    let fill = match placement.orientation {
        Orientation::Horizontal => Rect::new(t.x, t.y, t.w * frac, t.h),
        Orientation::Vertical => {
            let h = t.h * frac;
            Rect::new(t.x, t.bottom() - h, t.w, h)
        }
    };
    let filled = match placement.orientation {
        Orientation::Horizontal => fill.w,
        Orientation::Vertical => fill.h,
    };
    if filled > 0.0 {
        sink.emit(RenderCommand::FillRect {
            x: fill.x,
            y: fill.y,
            width: fill.w,
            height: fill.h,
            color: fade(look.fill, look.alpha),
            corner_radii: radius,
        });
    }
    let k = placement.thumb_rect(frac);
    sink.emit(RenderCommand::FillRect {
        x: k.x,
        y: k.y,
        width: k.w,
        height: k.h,
        color: fade(thumb, look.alpha),
        corner_radii: CornerRadii::all(k.w / 2.0),
    });
}

// ============================================================================
// The control
// ============================================================================

/// What became of a slider's value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderEvent {
    /// The value moved, and the drag moving it is still going on. Show it;
    /// do not save it -- it may yet be taken back.
    Changed(f64),
    /// The value is settled: a drag was let go of, or a key or the wheel moved
    /// it. Save it.
    Confirmed(f64),
    /// A drag was abandoned, and the value is back where the drag began --
    /// carried here, so a host that showed the drag's values can show this one.
    Cancelled(f64),
}

impl SliderEvent {
    /// The value the event carries, whichever it is.
    #[must_use]
    pub const fn value(self) -> f64 {
        match self {
            Self::Changed(v) | Self::Confirmed(v) | Self::Cancelled(v) => v,
        }
    }
}

/// What a slider made of an input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Response {
    /// Not the slider's: a key it has no use for, a press outside it. Pass it
    /// on.
    Ignored,
    /// The slider's, and the value is where it was: a thumb taken hold of, an
    /// arrow key at the end of the travel, the pointer moving over it.
    Taken,
    /// The slider's, and this is what became of the value.
    Event(SliderEvent),
}

impl Response {
    /// The event, if there was one.
    #[must_use]
    pub const fn event(self) -> Option<SliderEvent> {
        match self {
            Self::Event(e) => Some(e),
            Self::Ignored | Self::Taken => None,
        }
    }

    /// Whether the slider took the input -- so the host should not pass it on.
    #[must_use]
    pub const fn is_taken(self) -> bool {
        !matches!(self, Self::Ignored)
    }
}

/// A drag in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    /// The value when the press began: where Escape puts it back.
    origin: f64,
    /// Where on the thumb the press took hold, along the track's axis, from
    /// the thumb's centre. Keeping it is what stops the thumb jumping to
    /// centre itself under the pointer when pressed near its rim.
    grip: f32,
}

/// A value between two bounds, chosen by dragging a thumb, by the keyboard or
/// by the wheel.
///
/// Holds the value and the state of the gesture changing it, and nothing about
/// where it is drawn: that is a [`Placement`], which the host builds from its
/// own layout and hands to both [`draw`](Self::draw) and the input methods.
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    value: f64,
    min: f64,
    max: f64,
    /// The grid the value keeps to, and what an arrow key moves it by. Zero
    /// for a continuous slider.
    step: f64,
    /// What Page Up and Page Down move it by. Zero for the default.
    page: f64,
    state: DisabledState,
    drag: Option<Drag>,
    /// Whether the pointer is in the thumb's region.
    hover: bool,
    /// Wheel notches received and not yet a whole one: a precision trackpad
    /// sends fractions, and dropping each would leave the slider deaf to it.
    wheel_carry: f32,
}

impl Slider {
    /// A slider from `min` to `max`, at `value`, continuous until
    /// [`with_step`](Self::with_step) says otherwise.
    ///
    /// Non-finite bounds are read as zero and a `max` below `min` as `min`, so
    /// the slider has one position rather than none; a non-finite `value` is
    /// read as `min`. Nothing here can hold a NaN.
    #[must_use]
    pub fn new(min: f64, max: f64, value: f64) -> Self {
        let min = if min.is_finite() { min } else { 0.0 };
        let max = if max.is_finite() { max.max(min) } else { min };
        let mut slider = Self {
            value: min,
            min,
            max,
            step: 0.0,
            page: 0.0,
            state: DisabledState::Enabled,
            drag: None,
            hover: false,
            wheel_carry: 0.0,
        };
        slider.value = slider.snap(value);
        slider
    }

    /// Keep the value to multiples of `step` from `min` -- and to `max`,
    /// which is always reachable even when the range is not a whole number of
    /// steps. A step that is not a positive number means continuous.
    #[must_use]
    pub fn with_step(mut self, step: f64) -> Self {
        self.step = positive_or_zero(step);
        self.value = self.snap(self.value);
        self
    }

    /// Move by `page` for Page Up and Page Down, rather than a tenth of the
    /// range.
    #[must_use]
    pub fn with_page(mut self, page: f64) -> Self {
        self.page = positive_or_zero(page);
        self
    }

    /// The value.
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }

    /// The low end.
    #[must_use]
    pub const fn min(&self) -> f64 {
        self.min
    }

    /// The high end.
    #[must_use]
    pub const fn max(&self) -> f64 {
        self.max
    }

    /// How full the slider is, `0.0` at `min` and `1.0` at `max`.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a fraction in 0..=1; f32 holds it to far finer than a pixel"
        )]
        let frac = ((self.value - self.min) / span) as f32;
        clamp_frac(frac)
    }

    /// Put the value at `value`, kept to the range and the step.
    ///
    /// For a value that changed somewhere else -- the volume raised by a media
    /// key while the slider is on screen. Reports nothing: the host is the one
    /// telling. A drag in progress carries on from the new value.
    pub fn set_value(&mut self, value: f64) {
        self.value = self.snap(value);
    }

    /// Change the bounds, keeping the value inside them.
    pub fn set_range(&mut self, min: f64, max: f64) {
        let min = if min.is_finite() { min } else { 0.0 };
        self.min = min;
        self.max = if max.is_finite() { max.max(min) } else { min };
        self.value = self.snap(self.value);
    }

    /// Whether the slider may be used, and why not.
    #[must_use]
    pub const fn state(&self) -> &DisabledState {
        &self.state
    }

    /// Enable or disable the slider.
    ///
    /// Disabling it in the middle of a drag abandons the drag as Escape does,
    /// and the value goes back to where the drag began; the event says so, so
    /// that a host showing the drag's values can show that one instead.
    #[must_use = "a drag abandoned by disabling the slider reports the value it went back to"]
    pub fn set_state(&mut self, state: DisabledState) -> Option<SliderEvent> {
        let abandoned = if state.is_disabled() {
            self.hover = false;
            self.cancel().event()
        } else {
            None
        };
        self.state = state;
        abandoned
    }

    /// Whether a drag is in progress.
    #[must_use]
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether the pointer is where a press would take hold of the thumb.
    #[must_use]
    pub const fn is_hovered(&self) -> bool {
        self.hover
    }

    // ------------------------------------------------------------------
    // The value's arithmetic
    // ------------------------------------------------------------------

    /// `v` kept to the range and, for a stepped slider, to the nearest grid
    /// point -- where `max` counts as a grid point, so the top of a range that
    /// is not a whole number of steps can still be reached.
    fn snap(&self, v: f64) -> f64 {
        let v = if v.is_nan() {
            self.min
        } else {
            v.clamp(self.min, self.max)
        };
        if self.step <= 0.0 {
            return v;
        }
        let k = ((v - self.min) / self.step).round();
        let grid = (self.min + k * self.step).clamp(self.min, self.max);
        if self.max - v < (v - grid).abs() {
            self.max
        } else {
            grid
        }
    }

    /// What one arrow key moves the value by.
    fn line(&self) -> f64 {
        if self.step > 0.0 {
            self.step
        } else {
            (self.max - self.min) / CONTINUOUS_LINE_DIVISIONS
        }
    }

    /// What one Page Up moves the value by.
    fn page(&self) -> f64 {
        if self.page > 0.0 {
            self.page
        } else {
            self.line().max((self.max - self.min) / PAGE_DIVISIONS)
        }
    }

    /// The value `amount` up from the current one (`up`) or down.
    ///
    /// On a stepped slider this moves by whole steps to grid points, from
    /// wherever the value is: from a `max` that is off the grid, one step down
    /// is the grid point just below it, not a point a step's length away that
    /// would then round to the one below that.
    fn moved_by(&self, amount: f64, up: bool) -> f64 {
        if self.step <= 0.0 {
            let target = if up {
                self.value + amount
            } else {
                self.value - amount
            };
            return self.snap(target);
        }
        let steps = (amount / self.step).round().max(1.0);
        let k = (self.value - self.min) / self.step;
        let target_k = if up {
            (k + GRID_EPSILON).floor() + steps
        } else {
            (k - GRID_EPSILON).ceil() - steps
        };
        let target = self.min + target_k * self.step;
        if target >= self.max {
            self.max
        } else if target <= self.min {
            self.min
        } else {
            target
        }
    }

    /// Put the value at `target`, settled, and say what happened.
    fn settle_at(&mut self, target: f64) -> Response {
        if moved(self.value, target) {
            self.value = target;
            Response::Event(SliderEvent::Confirmed(target))
        } else {
            Response::Taken
        }
    }

    // ------------------------------------------------------------------
    // The pointer
    // ------------------------------------------------------------------

    /// A press of the primary button at `(x, y)`.
    ///
    /// Inside the thumb's region, takes hold of the thumb where it was pressed
    /// and moves nothing. Elsewhere on the slider, moves the thumb there and
    /// takes hold of it centred: `Changed`, the start of a drag. Outside the
    /// slider, or while it is disabled, `Ignored`.
    pub fn press(&mut self, placement: &Placement, x: f32, y: f32) -> Response {
        if self.state.is_disabled() || !placement.hit().contains(x, y) {
            return Response::Ignored;
        }
        let frac = self.fraction();
        let along = placement.along(x, y);
        if placement.thumb_hit(frac).contains(x, y) {
            let (cx, cy) = placement.centre_at(frac);
            self.drag = Some(Drag {
                origin: self.value,
                grip: along - placement.along(cx, cy),
            });
            return Response::Taken;
        }
        self.drag = Some(Drag {
            origin: self.value,
            grip: 0.0,
        });
        self.follow(placement, along)
    }

    /// The pointer moved to `(x, y)` with the button held. `Changed` if the
    /// value moved; `Ignored` if nothing is being dragged.
    pub fn drag_to(&mut self, placement: &Placement, x: f32, y: f32) -> Response {
        if self.drag.is_none() {
            return Response::Ignored;
        }
        self.follow(placement, placement.along(x, y))
    }

    /// Move the dragged thumb so its centre is at `along` less the grip.
    fn follow(&mut self, placement: &Placement, along: f32) -> Response {
        let grip = self.drag.map_or(0.0, |d| d.grip);
        let frac = placement.frac_at(along - grip);
        let target = self.snap(self.min + frac * (self.max - self.min));
        if moved(self.value, target) {
            self.value = target;
            Response::Event(SliderEvent::Changed(target))
        } else {
            Response::Taken
        }
    }

    /// The button was let go. `Confirmed` if the value is not where the drag
    /// began; `Taken` if it is; `Ignored` if nothing was being dragged.
    pub fn release(&mut self) -> Response {
        let Some(drag) = self.drag.take() else {
            return Response::Ignored;
        };
        if moved(drag.origin, self.value) {
            Response::Event(SliderEvent::Confirmed(self.value))
        } else {
            Response::Taken
        }
    }

    /// Abandon the drag: the value goes back to where it began. `Cancelled`
    /// with that value if the drag had moved it, `Taken` if not, `Ignored` if
    /// nothing was being dragged.
    ///
    /// Escape during a drag calls this ([`handle_key`](Self::handle_key)); so
    /// should a host whose window loses the pointer mid-drag.
    pub fn cancel(&mut self) -> Response {
        let Some(drag) = self.drag.take() else {
            return Response::Ignored;
        };
        if moved(drag.origin, self.value) {
            self.value = drag.origin;
            Response::Event(SliderEvent::Cancelled(drag.origin))
        } else {
            Response::Taken
        }
    }

    /// Act on a mouse event, in the space `placement` is in.
    ///
    /// Send every event, not only presses: a drag is a press, a run of moves
    /// and a release, and the moves also say where the pointer is for the
    /// thumb's hover light. The wheel is left alone -- see the module docs,
    /// and [`wheel`](Self::wheel).
    pub fn handle_mouse(&mut self, placement: &Placement, event: &MouseEvent) -> Response {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left)
            | MouseEventKind::DoubleClick(MouseButton::Left) => {
                // The second press of a quick pair arrives as a double-click;
                // on a slider it is simply another press.
                let r = self.press(placement, event.x, event.y);
                self.note_pointer(placement, event.x, event.y);
                r
            }
            MouseEventKind::Move | MouseEventKind::Enter => {
                let was = self.hover;
                self.note_pointer(placement, event.x, event.y);
                if self.drag.is_some() {
                    return self.drag_to(placement, event.x, event.y);
                }
                if was != self.hover || placement.hit().contains(event.x, event.y) {
                    Response::Taken
                } else {
                    Response::Ignored
                }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                let r = self.release();
                self.note_pointer(placement, event.x, event.y);
                r
            }
            MouseEventKind::Leave => {
                // Leaving is not letting go: a drag carries on, pinned to the
                // end the pointer left by, until the button comes up.
                self.hover = false;
                Response::Ignored
            }
            _ => Response::Ignored,
        }
    }

    /// Whether the pointer at `(x, y)` would take hold of the thumb.
    fn note_pointer(&mut self, placement: &Placement, x: f32, y: f32) {
        self.hover = !self.state.is_disabled()
            && (self.drag.is_some() || placement.thumb_hit(self.fraction()).contains(x, y));
    }

    // ------------------------------------------------------------------
    // The keyboard and the wheel
    // ------------------------------------------------------------------

    /// Act on a key.
    ///
    /// Right and Up raise the value a step, Left and Down lower it, Page Up and
    /// Page Down move a page, Home and End go to the ends: each `Confirmed`, or
    /// `Taken` at the end of the travel. During a drag, Escape abandons it
    /// and every other key is swallowed -- the pointer has the value. Keys
    /// with Ctrl, Alt or Super held, and every other key, are `Ignored`.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Response {
        if !key.pressed || self.state.is_disabled() {
            return Response::Ignored;
        }
        let m = key.modifiers;
        if m.ctrl || m.alt || m.super_key {
            return Response::Ignored;
        }
        if self.drag.is_some() {
            return if key.key == Key::Escape {
                self.cancel()
            } else {
                Response::Taken
            };
        }
        let target = match key.key {
            Key::Right | Key::Up => self.moved_by(self.line(), true),
            Key::Left | Key::Down => self.moved_by(self.line(), false),
            Key::PageUp => self.moved_by(self.page(), true),
            Key::PageDown => self.moved_by(self.page(), false),
            Key::Home => self.min,
            Key::End => self.max,
            _ => return Response::Ignored,
        };
        self.settle_at(target)
    }

    /// A turn of the wheel, in notches (positive away from the user, which
    /// raises the value). One notch is one arrow key's step; fractions of a
    /// notch are kept until they add up to one.
    ///
    /// Send it only while the slider has the keyboard focus: see the module
    /// docs.
    pub fn wheel(&mut self, notches: f32) -> Response {
        if self.state.is_disabled() || self.drag.is_some() || !notches.is_finite() {
            return Response::Ignored;
        }
        self.wheel_carry += notches;
        let whole = self.wheel_carry.trunc();
        self.wheel_carry -= whole;
        if whole == 0.0 {
            return Response::Taken;
        }
        let amount = self.line() * f64::from(whole.abs());
        let target = self.moved_by(amount, whole > 0.0);
        self.settle_at(target)
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    /// Draw the slider at `placement`.
    ///
    /// `focused` is whether it has the keyboard, which the host knows and the
    /// slider does not; `focus_ring` is the ring's width, which a host that
    /// scales its lines for accessibility passes scaled. A disabled slider is
    /// drawn at the toolkit's disabled opacity, with no light and no ring.
    pub fn draw(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        placement: &Placement,
        look: Look,
        focused: bool,
        focus_ring: f32,
    ) {
        let frac = self.fraction();
        if self.state.is_disabled() {
            let dimmed = Look {
                alpha: scaled_alpha(look.alpha, DISABLED_OPACITY),
                ..look
            };
            paint_bar(sink, placement, frac, dimmed, p.text);
            return;
        }
        let k = placement.thumb_rect(frac);
        if self.hover || self.drag.is_some() {
            let d = k.w + HALO * 2.0;
            sink.emit(RenderCommand::FillRect {
                x: k.x - HALO,
                y: k.y - HALO,
                width: d,
                height: d,
                color: Color::rgba(
                    look.fill.r,
                    look.fill.g,
                    look.fill.b,
                    scaled_alpha(look.alpha, f32::from(HALO_ALPHA) / 255.0),
                ),
                corner_radii: CornerRadii::all(d / 2.0),
            });
        }
        paint_bar(sink, placement, frac, look, p.text);
        if focused && focus_ring > 0.0 && focus_ring.is_finite() {
            // Outside the thumb with a gap, so the ring reads as a ring round
            // the handle rather than a thicker rim on it.
            let reach = FOCUS_GAP + focus_ring;
            let d = k.w + reach * 2.0;
            sink.emit(RenderCommand::StrokeRect {
                x: k.x - reach,
                y: k.y - reach,
                width: d,
                height: d,
                color: fade(p.accent, look.alpha),
                line_width: focus_ring,
                corner_radii: CornerRadii::all(d / 2.0),
            });
        }
    }
}

/// `v` if it is a positive finite number, otherwise zero.
fn positive_or_zero(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Whether a value moved from `from` to `to`.
///
/// Exact, on purpose: the question is whether the stored number changed, not
/// whether two computations came out close. Every value this module stores has
/// been through [`Slider::snap`], so two that should be equal are.
#[allow(
    clippy::float_cmp,
    reason = "an exact comparison of stored values is the question being asked"
)]
fn moved(from: f64, to: f64) -> bool {
    from != to
}

#[cfg(test)]
mod tests {
    // A helper handed the wrong command shape has nothing useful to return,
    // and the panic names what it got instead; indexing a render this module
    // just built is the same argument.
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::event::Modifiers;
    use crate::palette::readable_on;
    use crate::theme::contrast_ratio;

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// The four shapes the desktop draws, as `(track thickness, thumb)`.
    const SHAPES: [(f32, f32); 4] = [(6.0, 12.0), (6.0, 10.0), (6.0, 14.0), (4.0, 12.0)];

    fn rect(c: &RenderCommand) -> (f32, f32, f32, f32, Color, f32) {
        match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            } => (*x, *y, *width, *height, *color, corner_radii.top_left),
            other => panic!("expected a FillRect, got {other:?}"),
        }
    }

    fn bar(p: &Palette, height: f32, thumb: f32, frac: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        draw_bar(
            &mut cmds,
            p,
            &Placement::horizontal(Rect::new(100.0, 50.0, 150.0, height), thumb),
            frac,
            Look {
                track: p.surface1,
                fill: p.accent,
                alpha: u8::MAX,
            },
        );
        cmds
    }

    /// Every hue the palette can be asked to make its accent.
    fn accents(p: &Palette) -> [Color; 14] {
        [
            p.blue,
            p.green,
            p.red,
            p.yellow,
            p.peach,
            p.lavender,
            p.mauve,
            p.sapphire,
            p.teal,
            p.sky,
            p.pink,
            p.rosewater,
            p.flamingo,
            p.maroon,
        ]
    }

    /// A horizontal slider 0..=100 on a 200-pixel track at (100, 50), 4 thick,
    /// with a 12-pixel thumb.
    fn placed() -> Placement {
        Placement::horizontal(Rect::new(100.0, 50.0, 200.0, 4.0), 12.0)
    }

    /// The x at which the thumb's centre sits for `value` on [`placed`].
    fn x_of(value: f64) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let v = value as f32;
        100.0 + 200.0 * v / 100.0
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn mouse(kind: MouseEventKind, x: f32, y: f32) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    // ------------------------------------------------------------------
    // Drawing: the geometry and the inks, moved with the desktop's module
    // ------------------------------------------------------------------

    /// The pixels the desktop's five hand-written sliders drew, recovered from
    /// them before they were replaced. If the rule were wrong, moving them onto
    /// it would have moved something on screen.
    #[test]
    fn the_geometry_is_the_one_every_hand_written_slider_already_used() {
        let p = Palette::for_mode(false);
        for (height, thumb) in SHAPES {
            for frac in [0.0_f32, 0.25, 1.0] {
                let cmds = bar(&p, height, thumb, frac);
                let (tx, ty, tw, th, _, tr) = rect(&cmds[0]);
                assert_eq!((tx, ty, tw, th), (100.0, 50.0, 150.0, height));
                assert!(
                    (tr - height / 2.0).abs() < f32::EPSILON,
                    "a track {height} thick has radius {tr}"
                );
                let fill_w = 150.0 * frac;
                if frac > 0.0 {
                    let (fx, fy, fw, fh, _, fr) = rect(&cmds[1]);
                    assert_eq!((fx, fy, fh), (100.0, 50.0, height));
                    assert!((fw - fill_w).abs() < f32::EPSILON, "fill width {fw}");
                    assert!((fr - height / 2.0).abs() < f32::EPSILON);
                }
                let (kx, ky, kw, kh, _, kr) = rect(cmds.last().expect("a thumb"));
                assert_eq!((kw, kh), (thumb, thumb), "the thumb is square");
                assert!(
                    (kr - thumb / 2.0).abs() < f32::EPSILON,
                    "the thumb is round"
                );
                assert!(
                    (kx + thumb / 2.0 - (100.0 + fill_w)).abs() < 1e-3,
                    "the thumb is centred on the end of the fill"
                );
                assert!(
                    (ky + thumb / 2.0 - (50.0 + height / 2.0)).abs() < f32::EPSILON,
                    "the thumb is centred on the track's midline"
                );
            }
        }
    }

    /// The premise of the ink rule, asserted rather than assumed: the thumb
    /// hangs over the track on both sides, so what is behind most of it is the
    /// card, not the fill.
    #[test]
    fn the_thumb_overhangs_the_track_on_every_shape_the_desktop_draws() {
        let p = Palette::for_mode(false);
        for (height, thumb) in SHAPES {
            let cmds = bar(&p, height, thumb, 0.5);
            let (_, ky, _, kh, ..) = rect(cmds.last().expect("a thumb"));
            assert!(
                ky < 50.0 && ky + kh > 50.0 + height,
                "a {thumb}px thumb on a {height}px track spans {ky}..{}",
                ky + kh
            );
        }
    }

    /// The thumb is `text`, whatever the fill, and `text` is what a card can
    /// show: at least 3:1 (WCAG SC 1.4.11, for a graphical object that
    /// identifies a control) against every card a slider sits on, in both
    /// modes. The fill is never the thumb's colour -- the defect that made one
    /// of the desktop's copies vanish for half its travel.
    #[test]
    fn the_thumb_is_legible_against_every_card_it_can_sit_on() {
        let mut worst: Option<(String, f32)> = None;
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            for (name, card) in [
                ("base", p.base),
                ("mantle", p.mantle),
                ("crust", p.crust),
                ("surface0", p.surface0),
                ("surface1", p.surface1),
                ("surface2", p.surface2),
            ] {
                let c = contrast_ratio(card, p.text);
                if worst.as_ref().is_none_or(|(_, w)| c < *w) {
                    worst = Some((format!("`{name}` in {mode} mode"), c));
                }
            }
            for fill in accents(&p) {
                let mut cmds = Vec::new();
                draw_bar(
                    &mut cmds,
                    &p,
                    &Placement::horizontal(Rect::new(0.0, 0.0, 100.0, 6.0), 12.0),
                    0.5,
                    Look {
                        track: p.surface1,
                        fill,
                        alpha: u8::MAX,
                    },
                );
                let (.., ink, _) = rect(cmds.last().expect("a thumb"));
                assert_eq!(
                    ink, p.text,
                    "the thumb over a {fill:?} fill in {mode} mode is not `text`"
                );
                assert_ne!(
                    ink, fill,
                    "the thumb over a {fill:?} fill in {mode} mode is that fill"
                );
            }
        }
        let (where_, c) = worst.expect("there are cards");
        assert!(c >= 3.0, "the tightest thumb is on {where_}, at {c:.2}:1");
    }

    /// The reasoning in the module docs, stated as a number so nobody
    /// "corrects" the difference from the switch's knob by hand: `text` beats
    /// `readable_on(fill)` against the card that defines the thumb's outline.
    #[test]
    fn text_beats_readable_on_the_fill_against_the_card() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for fill in accents(&p) {
                let mine = contrast_ratio(p.base, p.text);
                let theirs = contrast_ratio(p.base, readable_on(fill));
                assert!(
                    mine > theirs,
                    "on a {fill:?} fill in {} mode, `text` is {mine:.2}:1 against the card and \
                     `readable_on(fill)` {theirs:.2}:1",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// An empty fill draws nothing; any fill at all draws a rectangle.
    #[test]
    fn a_slider_on_its_floor_emits_no_fill_rectangle() {
        let p = Palette::for_mode(false);
        assert_eq!(bar(&p, 6.0, 12.0, 0.0).len(), 2, "track and thumb only");
        assert_eq!(bar(&p, 6.0, 12.0, 0.001).len(), 3, "the fill is drawn");
    }

    /// A fraction outside `0..=1`, or not a number, still leaves the thumb on
    /// the track.
    #[test]
    fn an_out_of_range_fraction_cannot_push_the_thumb_off_the_track() {
        let p = Palette::for_mode(false);
        for (frac, expected) in [(-3.0_f32, 100.0_f32), (7.0, 250.0), (f32::NAN, 100.0)] {
            let cmds = bar(&p, 6.0, 12.0, frac);
            let (kx, ..) = rect(cmds.last().expect("a thumb"));
            assert!(
                (kx + 6.0 - expected).abs() < f32::EPSILON,
                "frac {frac} put the thumb at {}",
                kx + 6.0
            );
        }
    }

    /// An overlay fades as one thing: all three colours take the alpha once,
    /// and an opaque slider keeps the roles themselves.
    #[test]
    fn the_alpha_reaches_every_part_of_the_control() {
        let p = Palette::for_mode(false);
        let mut faded = Vec::new();
        draw_bar(
            &mut faded,
            &p,
            &Placement::horizontal(Rect::new(0.0, 0.0, 100.0, 6.0), 10.0),
            0.5,
            Look {
                track: p.surface0,
                fill: p.accent,
                alpha: 128,
            },
        );
        for (c, role) in faded.iter().zip([p.surface0, p.accent, p.text]) {
            let (.., color, _) = rect(c);
            assert_eq!(color, Color::rgba(role.r, role.g, role.b, 128));
        }
        let opaque = bar(&p, 6.0, 12.0, 0.5);
        let (.., c, _) = rect(&opaque[0]);
        assert_eq!(c, p.surface1);
    }

    /// A vertical slider fills from the bottom, and its thumb rises with the
    /// value.
    #[test]
    fn a_vertical_slider_fills_from_the_bottom() {
        let p = Palette::for_mode(false);
        let placement = Placement::vertical(Rect::new(20.0, 100.0, 6.0, 200.0), 12.0);
        let mut cmds = Vec::new();
        draw_bar(
            &mut cmds,
            &p,
            &placement,
            0.25,
            Look::accent(&p, p.surface1),
        );
        let (fx, fy, fw, fh, ..) = rect(&cmds[1]);
        assert_eq!((fx, fw), (20.0, 6.0));
        assert!(
            (fh - 50.0).abs() < 1e-4 && (fy + fh - 300.0).abs() < 1e-4,
            "fill {fy}+{fh}"
        );
        let (kx, ky, kw, ..) = rect(&cmds[2]);
        assert!(
            (ky + 6.0 - 250.0).abs() < 1e-4,
            "thumb centre y {}",
            ky + 6.0
        );
        assert!(
            (kx + kw / 2.0 - 23.0).abs() < 1e-4,
            "thumb centred across the track"
        );
    }

    /// Hover and drag light the thumb; focus rings it; disabled dims all of
    /// it and does neither.
    #[test]
    fn hover_lights_focus_rings_and_disabled_dims() {
        let p = Palette::for_mode(false);
        let placement = placed();
        let look = Look::accent(&p, p.surface1);
        let mut s = Slider::new(0.0, 100.0, 50.0);

        let mut plain = Vec::new();
        s.draw(&mut plain, &p, &placement, look, false, 2.0);
        assert_eq!(plain.len(), 3, "track, fill, thumb: {plain:?}");

        // The pointer beside the thumb, outside its drawn circle but inside
        // its region.
        let r = s.handle_mouse(
            &placement,
            &mouse(MouseEventKind::Move, x_of(50.0) + 9.0, 52.0),
        );
        assert_eq!(r, Response::Taken);
        assert!(s.is_hovered());
        let mut lit = Vec::new();
        s.draw(&mut lit, &p, &placement, look, false, 2.0);
        assert_eq!(lit.len(), 4, "the light is drawn first: {lit:?}");
        let (hx, _, hw, _, hc, _) = rect(&lit[0]);
        assert!(
            (hw - (12.0 + HALO * 2.0)).abs() < 1e-4 && (hx + hw / 2.0 - x_of(50.0)).abs() < 1e-4
        );
        assert!(hc.a < u8::MAX, "the light is translucent");

        let mut ringed = Vec::new();
        s.draw(&mut ringed, &p, &placement, look, true, 2.0);
        assert!(
            matches!(ringed.last(), Some(RenderCommand::StrokeRect { color, line_width, .. })
                if *color == p.accent && (*line_width - 2.0).abs() < f32::EPSILON),
            "a focused slider rings its thumb in the accent: {ringed:?}"
        );

        assert!(
            s.set_state(DisabledState::Disabled { reason: None })
                .is_none()
        );
        let mut dim = Vec::new();
        s.draw(&mut dim, &p, &placement, look, true, 2.0);
        assert_eq!(dim.len(), 3, "no light, no ring: {dim:?}");
        for c in &dim {
            let (.., color, _) = rect(c);
            assert!(color.a < u8::MAX, "every part is dimmed: {color:?}");
        }
    }

    // ------------------------------------------------------------------
    // The regions
    // ------------------------------------------------------------------

    /// A four-pixel track is a 24-pixel target across, and the half-thumb
    /// overhang at each end is inside it with a margin to spare.
    #[test]
    fn the_whole_slider_is_a_generous_target() {
        let hit = placed().hit();
        assert!(hit.h >= grab::MIN_TARGET, "{hit:?}");
        assert!(hit.x <= 100.0 - 6.0 - grab::HANDLE_MARGIN, "{hit:?}");
        assert!(hit.right() >= 300.0 + 6.0 + grab::HANDLE_MARGIN, "{hit:?}");
        assert!(
            (hit.y + hit.h / 2.0 - 52.0).abs() < 1e-4,
            "centred on the track: {hit:?}"
        );

        let v = Placement::vertical(Rect::new(20.0, 100.0, 4.0, 200.0), 12.0).hit();
        assert!(
            v.w >= grab::MIN_TARGET && v.y <= 100.0 - 6.0 && v.bottom() >= 306.0,
            "{v:?}"
        );
    }

    /// The thumb's region is at least the minimum target, round the thumb.
    #[test]
    fn the_thumb_is_a_generous_target() {
        let r = placed().thumb_hit(0.5);
        assert!(r.w >= grab::MIN_TARGET && r.h >= grab::MIN_TARGET, "{r:?}");
        assert!((r.x + r.w / 2.0 - 200.0).abs() < 1e-4, "{r:?}");
    }

    // ------------------------------------------------------------------
    // The pointer
    // ------------------------------------------------------------------

    /// A press beside the thumb -- outside what is drawn, inside its region --
    /// takes hold of it without moving it, and the drag then moves it by
    /// exactly as far as the pointer moves.
    #[test]
    fn a_press_near_the_thumb_takes_hold_without_a_jump() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0);
        let beside = x_of(50.0) + 10.0; // four pixels past the drawn rim
        assert_eq!(s.press(&placement, beside, 52.0), Response::Taken);
        assert!(s.is_dragging());
        assert!(close(s.value(), 50.0), "nothing moved: {}", s.value());

        // Twenty pixels right is ten units on a 200-pixel track of 100.
        let r = s.drag_to(&placement, beside + 20.0, 52.0);
        assert_eq!(r, Response::Event(SliderEvent::Changed(s.value())));
        assert!(
            close(s.value(), 60.0),
            "the thumb kept its grip: {}",
            s.value()
        );
    }

    /// A press on the track away from the thumb moves it there and holds it
    /// centred.
    #[test]
    fn a_press_on_the_track_jumps_there_and_holds_it() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        let r = s.press(&placement, x_of(20.0), 52.0);
        assert_eq!(r, Response::Event(SliderEvent::Changed(20.0)));
        assert!(s.is_dragging());
        let r = s.drag_to(&placement, x_of(30.0), 52.0);
        assert_eq!(
            r.event().map(SliderEvent::value).map(|v| close(v, 30.0)),
            Some(true),
            "{r:?}"
        );
    }

    /// The generous region is what answers: a press eight pixels above a
    /// four-pixel track still lands, and one well clear of it does not.
    #[test]
    fn a_press_just_off_the_drawn_track_still_lands() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0);
        assert!(
            s.press(&placement, x_of(20.0), 44.0).is_taken(),
            "8px above the track's top"
        );
        assert!(s.release().is_taken());
        assert_eq!(
            s.press(&placement, x_of(20.0), 20.0),
            Response::Ignored,
            "30px above"
        );
        assert!(!s.is_dragging());
        // Past the right end by the thumb's overhang: the thumb at 100 is
        // there, and it can be taken hold of.
        let mut full = Slider::new(0.0, 100.0, 100.0);
        assert_eq!(full.press(&placement, 300.0 + 8.0, 52.0), Response::Taken);
    }

    /// Letting go confirms a value that moved and nothing if it did not.
    #[test]
    fn release_confirms_only_a_moved_value() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        assert_eq!(s.press(&placement, x_of(50.0), 52.0), Response::Taken);
        assert_eq!(
            s.release(),
            Response::Taken,
            "a click on the thumb changed nothing"
        );

        assert!(s.press(&placement, x_of(50.0), 52.0).is_taken());
        assert!(s.drag_to(&placement, x_of(70.0), 52.0).event().is_some());
        assert_eq!(
            s.release(),
            Response::Event(SliderEvent::Confirmed(s.value()))
        );
        assert!(close(s.value(), 70.0));
        assert!(!s.is_dragging());
        assert_eq!(s.release(), Response::Ignored, "nothing to let go of");

        // Out and back to where it began: nothing to save.
        assert!(s.press(&placement, x_of(70.0), 52.0).is_taken());
        assert!(s.drag_to(&placement, x_of(90.0), 52.0).event().is_some());
        assert!(s.drag_to(&placement, x_of(70.0), 52.0).event().is_some());
        assert_eq!(s.release(), Response::Taken);
    }

    /// Escape during a drag puts the value back and says where; without a
    /// drag it is not the slider's.
    #[test]
    fn escape_during_a_drag_puts_the_value_back() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        assert_eq!(s.handle_key(&key(Key::Escape)), Response::Ignored);
        assert!(s.press(&placement, x_of(50.0), 52.0).is_taken());
        assert!(s.drag_to(&placement, x_of(80.0), 52.0).event().is_some());
        assert_eq!(
            s.handle_key(&key(Key::Escape)),
            Response::Event(SliderEvent::Cancelled(50.0))
        );
        assert!(close(s.value(), 50.0));
        assert!(!s.is_dragging());
        assert_eq!(s.release(), Response::Ignored, "the drag is over");
    }

    /// Dragged past either end, the thumb stays at that end.
    #[test]
    fn a_drag_past_an_end_stops_at_the_end() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        assert!(s.press(&placement, x_of(50.0), 52.0).is_taken());
        assert_eq!(
            s.drag_to(&placement, -500.0, 52.0),
            Response::Event(SliderEvent::Changed(0.0))
        );
        assert_eq!(
            s.drag_to(&placement, 5000.0, 900.0),
            Response::Event(SliderEvent::Changed(100.0))
        );
        assert_eq!(
            s.drag_to(&placement, 6000.0, 52.0),
            Response::Taken,
            "already at the end"
        );
    }

    /// The whole mouse stream -- press, moves, release -- through one call.
    #[test]
    fn handle_mouse_runs_a_whole_drag() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 0.0).with_step(1.0);
        let r = s.handle_mouse(
            &placement,
            &mouse(MouseEventKind::Press(MouseButton::Left), x_of(40.0), 52.0),
        );
        assert_eq!(r, Response::Event(SliderEvent::Changed(40.0)));
        let r = s.handle_mouse(&placement, &mouse(MouseEventKind::Move, x_of(45.0), 52.0));
        assert_eq!(r, Response::Event(SliderEvent::Changed(45.0)));
        // The pointer leaves the slider's region, still held: the drag goes on.
        assert_eq!(
            s.handle_mouse(&placement, &mouse(MouseEventKind::Leave, 0.0, 0.0)),
            Response::Ignored
        );
        let r = s.handle_mouse(&placement, &mouse(MouseEventKind::Move, x_of(60.0), 400.0));
        assert_eq!(r, Response::Event(SliderEvent::Changed(60.0)));
        let r = s.handle_mouse(
            &placement,
            &mouse(
                MouseEventKind::Release(MouseButton::Left),
                x_of(60.0),
                400.0,
            ),
        );
        assert_eq!(r, Response::Event(SliderEvent::Confirmed(60.0)));
        // A move far from it afterwards is not its business.
        assert_eq!(
            s.handle_mouse(&placement, &mouse(MouseEventKind::Move, 900.0, 400.0)),
            Response::Ignored
        );
        // Nor is the wheel, whose door is `wheel`.
        let wheel = mouse(
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
            x_of(60.0),
            52.0,
        );
        assert_eq!(s.handle_mouse(&placement, &wheel), Response::Ignored);
        assert!(close(s.value(), 60.0));
    }

    /// The second press of a quick pair arrives as a double-click, and it is
    /// still a press.
    #[test]
    fn a_double_click_is_a_press() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        let r = s.handle_mouse(
            &placement,
            &mouse(
                MouseEventKind::DoubleClick(MouseButton::Left),
                x_of(10.0),
                52.0,
            ),
        );
        assert_eq!(r, Response::Event(SliderEvent::Changed(10.0)));
        assert!(s.is_dragging());
    }

    /// A vertical slider rises as the pointer goes up.
    #[test]
    fn a_vertical_drag_raises_the_value_going_up() {
        let placement = Placement::vertical(Rect::new(20.0, 100.0, 4.0, 200.0), 12.0);
        let mut s = Slider::new(0.0, 100.0, 0.0).with_step(1.0);
        // y = 150 is three quarters of the way up a track from 300 to 100.
        assert_eq!(
            s.press(&placement, 22.0, 150.0),
            Response::Event(SliderEvent::Changed(75.0))
        );
        assert_eq!(
            s.drag_to(&placement, 22.0, 250.0),
            Response::Event(SliderEvent::Changed(25.0))
        );
    }

    /// A disabled slider takes nothing, and disabling one mid-drag puts the
    /// value back and says so.
    #[test]
    fn a_disabled_slider_takes_nothing() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0);
        assert!(s.press(&placement, x_of(50.0), 52.0).is_taken());
        assert!(s.drag_to(&placement, x_of(90.0), 52.0).event().is_some());
        let back = s.set_state(DisabledState::Disabled {
            reason: Some("muted".into()),
        });
        assert_eq!(back, Some(SliderEvent::Cancelled(50.0)));
        assert!(!s.is_dragging() && !s.is_hovered());
        assert_eq!(s.press(&placement, x_of(20.0), 52.0), Response::Ignored);
        assert_eq!(s.handle_key(&key(Key::Right)), Response::Ignored);
        assert_eq!(s.wheel(1.0), Response::Ignored);
        assert!(close(s.value(), 50.0));
        assert_eq!(s.set_state(DisabledState::Enabled), None);
        assert!(s.handle_key(&key(Key::Right)).event().is_some());
    }

    // ------------------------------------------------------------------
    // The keyboard and the wheel
    // ------------------------------------------------------------------

    /// The WAI-ARIA keys, each settling the value.
    #[test]
    fn the_keys_move_by_steps_pages_and_to_the_ends() {
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(5.0);
        let expect = |s: &mut Slider, k: Key, v: f64| {
            assert_eq!(
                s.handle_key(&key(k)),
                Response::Event(SliderEvent::Confirmed(v)),
                "{k:?}"
            );
        };
        expect(&mut s, Key::Right, 55.0);
        expect(&mut s, Key::Up, 60.0);
        expect(&mut s, Key::Left, 55.0);
        expect(&mut s, Key::Down, 50.0);
        expect(&mut s, Key::PageUp, 60.0);
        expect(&mut s, Key::PageDown, 50.0);
        expect(&mut s, Key::End, 100.0);
        assert_eq!(
            s.handle_key(&key(Key::Right)),
            Response::Taken,
            "at the end of the travel"
        );
        expect(&mut s, Key::Home, 0.0);
        assert_eq!(s.handle_key(&key(Key::Down)), Response::Taken);
    }

    /// Shortcuts and keys a slider has no use for are passed on.
    #[test]
    fn other_keys_are_passed_on() {
        let mut s = Slider::new(0.0, 100.0, 50.0);
        assert_eq!(s.handle_key(&key(Key::Tab)), Response::Ignored);
        assert_eq!(s.handle_key(&key(Key::Enter)), Response::Ignored);
        let mut ctrl = key(Key::Right);
        ctrl.modifiers.ctrl = true;
        assert_eq!(s.handle_key(&ctrl), Response::Ignored);
        let mut released = key(Key::Right);
        released.pressed = false;
        assert_eq!(s.handle_key(&released), Response::Ignored);
        assert!(close(s.value(), 50.0));
    }

    /// During a drag the pointer has the value: keys other than Escape are
    /// swallowed and change nothing.
    #[test]
    fn keys_during_a_drag_change_nothing() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        assert!(s.press(&placement, x_of(50.0), 52.0).is_taken());
        assert_eq!(s.handle_key(&key(Key::Right)), Response::Taken);
        assert_eq!(s.handle_key(&key(Key::End)), Response::Taken);
        assert!(close(s.value(), 50.0));
        assert!(s.is_dragging());
    }

    /// A continuous slider steps a hundredth of its range per key and a tenth
    /// per page.
    #[test]
    fn a_continuous_slider_steps_by_fractions_of_its_range() {
        let mut s = Slider::new(0.0, 2.0, 1.0);
        let r = s.handle_key(&key(Key::Right));
        assert!(close(r.event().expect("moved").value(), 1.02), "{r:?}");
        let r = s.handle_key(&key(Key::PageDown));
        assert!(close(r.event().expect("moved").value(), 0.82), "{r:?}");
    }

    /// A range that is not a whole number of steps: the top is still
    /// reachable, and one step down from it is the grid point just below.
    #[test]
    fn the_top_of_an_uneven_range_is_reachable_and_steps_back_onto_the_grid() {
        let placement = placed();
        let mut s = Slider::new(0.0, 10.0, 9.0).with_step(3.0);
        assert_eq!(
            s.handle_key(&key(Key::Right)),
            Response::Event(SliderEvent::Confirmed(10.0))
        );
        assert_eq!(
            s.handle_key(&key(Key::Left)),
            Response::Event(SliderEvent::Confirmed(9.0))
        );
        assert_eq!(
            s.handle_key(&key(Key::Left)),
            Response::Event(SliderEvent::Confirmed(6.0))
        );
        // Dragged to the far end, it lands on 10, not the last grid point.
        let mut d = Slider::new(0.0, 10.0, 0.0).with_step(3.0);
        assert!(d.press(&placement, 100.0, 52.0).is_taken());
        assert_eq!(
            d.drag_to(&placement, 299.0, 52.0),
            Response::Event(SliderEvent::Changed(10.0))
        );
    }

    /// Steps that do not divide evenly in binary still step one at a time.
    #[test]
    fn a_decimal_step_does_not_skip_or_stick() {
        let mut s = Slider::new(0.0, 1.0, 0.0).with_step(0.1);
        for i in 1..=10 {
            let r = s.handle_key(&key(Key::Right));
            let v = r.event().expect("each press moves").value();
            assert!(close(v, f64::from(i) / 10.0), "press {i} gave {v}");
        }
        for i in (0..10).rev() {
            let v = s
                .handle_key(&key(Key::Left))
                .event()
                .expect("moves")
                .value();
            assert!(close(v, f64::from(i) / 10.0), "back to {i}/10 gave {v}");
        }
    }

    /// A drag snaps to the step.
    #[test]
    fn a_drag_snaps_to_the_step() {
        let placement = placed();
        let mut s = Slider::new(0.0, 100.0, 0.0).with_step(10.0);
        // 23 units along snaps to 20; 27 to 30.
        assert_eq!(
            s.press(&placement, x_of(23.0), 52.0),
            Response::Event(SliderEvent::Changed(20.0))
        );
        assert_eq!(
            s.drag_to(&placement, x_of(27.0), 52.0),
            Response::Event(SliderEvent::Changed(30.0))
        );
        assert_eq!(
            s.drag_to(&placement, x_of(33.0), 52.0),
            Response::Taken,
            "still 30"
        );
    }

    /// One notch is one step; fractions add up; away from the user raises.
    #[test]
    fn the_wheel_steps_and_keeps_fractions() {
        let mut s = Slider::new(0.0, 100.0, 50.0).with_step(1.0);
        assert_eq!(s.wheel(1.0), Response::Event(SliderEvent::Confirmed(51.0)));
        assert_eq!(s.wheel(-2.0), Response::Event(SliderEvent::Confirmed(49.0)));
        assert_eq!(s.wheel(0.4), Response::Taken);
        assert_eq!(s.wheel(0.4), Response::Taken);
        assert_eq!(
            s.wheel(0.4),
            Response::Event(SliderEvent::Confirmed(50.0)),
            "1.2 notches"
        );
        assert_eq!(s.wheel(f32::NAN), Response::Ignored);
    }

    // ------------------------------------------------------------------
    // What a caller can hand it
    // ------------------------------------------------------------------

    /// Bad bounds and values become a slider with a place to be, never a NaN.
    #[test]
    fn nonsense_in_is_a_usable_slider_out() {
        let s = Slider::new(f64::NAN, f64::INFINITY, f64::NAN);
        assert!(s.value().is_finite() && s.min().is_finite() && s.max().is_finite());
        assert!(close(s.fraction().into(), 0.0));

        let s = Slider::new(10.0, 0.0, 5.0);
        assert!(
            close(s.max(), 10.0) && close(s.value(), 10.0),
            "an inverted range is one position"
        );

        let mut s = Slider::new(0.0, 100.0, 250.0);
        assert!(close(s.value(), 100.0), "clamped in");
        s.set_value(f64::NAN);
        assert!(close(s.value(), 0.0));
        s.set_range(0.0, 10.0);
        s.set_value(7.0);
        s.set_range(0.0, 5.0);
        assert!(
            close(s.value(), 5.0),
            "a shrinking range carries the value in"
        );

        let s = Slider::new(0.0, 10.0, 3.0)
            .with_step(-1.0)
            .with_page(f64::NAN);
        assert!(close(s.value(), 3.0), "a nonsense step is continuous");
    }

    /// A track with no length has one position, and no division by its
    /// length happens.
    #[test]
    fn a_track_of_no_length_has_one_position() {
        let placement = Placement::horizontal(Rect::new(50.0, 50.0, 0.0, 4.0), 12.0);
        let mut s = Slider::new(0.0, 100.0, 50.0);
        let r = s.press(&placement, 52.0, 52.0);
        assert!(r.is_taken(), "{r:?}");
        let r = s.drag_to(&placement, 90.0, 52.0);
        assert!(s.value().is_finite(), "{r:?}");
    }
}
