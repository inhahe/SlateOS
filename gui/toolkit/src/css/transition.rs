//! Transitions: a widget's style moving from what it was to what it has
//! become over the time its `transition` gives, rather than jumping.
//!
//! # What moves
//!
//! The values a style has that can be part-way between two others
//! ([`Animated`]): colours, opacity, lengths -- padding, margins, borders'
//! widths and corners' radii, sizes and their limits, the font's size --
//! the line height, and shadows. A family, a weight, an alignment, a
//! border's style and a pointer cannot be part-way and change at once, as
//! in CSS. So does a value with no number to move from: a size that was
//! `auto`, or a text colour the program left to the theme.
//!
//! # The desktop's motion
//!
//! A program's transitions follow the user's motion setting
//! ([`crate::motion::Motion`], `design-decisions.md` §1446) as the
//! desktop's own do: a duration is stated against the standard transition
//! and scaled with the rest ([`Motion::duration_ms`]), and with animations
//! off nothing moves -- a change is shown at once. A transition that names
//! no timing function follows the desktop's curve ([`Timing::Desktop`]),
//! not CSS's `ease`, so a program moves as the desktop around it does
//! unless it asks otherwise (`design-decisions.md` §1478).
//!
//! # When a value changes
//!
//! [`Transitions::update`] is given each style as it is computed. A value
//! that differs from the last one computed starts a transition from what is
//! shown at that moment -- part-way through another, if one was running --
//! to the new one; a change back to where a running transition started is
//! CSS's reversal, made as much shorter as the way back is.
//! A style with no `transition` for a value changes it at once, which is
//! CSS's rule too: it is the style changed *to* whose `transition` counts.
//!
//! A box's length given as a percentage of its container is known only
//! where the container lays it out, and is moved there
//! ([`Transitions::settle`]).

use std::collections::BTreeMap;

use super::compute::BoxLengths;
use super::decl::{Corner, Property, Side};
use crate::color::Color;
use crate::motion::Motion;
use crate::style::{BoxShadow, Style};

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// How a transition's progress is eased: CSS's timing functions, and the
/// desktop's own curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    /// The desktop's motion curve ([`Motion::arriving`]): what a
    /// transition naming no timing function follows.
    Desktop,
    /// `linear`: a constant speed.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`: `ease` and the other keywords are
    /// ones of these ([`Timing::EASE`] ...). Each `x` is from 0 to 1.
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, position)`: `n` jumps, where `position` says.
    Steps(u16, StepPosition),
}

/// Where a `steps()` timing function jumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start` (`start`): at the start of each step.
    JumpStart,
    /// `jump-end` (`end`): at the end of each step.
    JumpEnd,
    /// `jump-none`: at neither end -- the first step is the start's value,
    /// the last the end's.
    JumpNone,
    /// `jump-both`: at both ends.
    JumpBoth,
}

impl Timing {
    /// `ease`.
    pub const EASE: Self = Self::CubicBezier(0.25, 0.1, 0.25, 1.0);
    /// `ease-in`.
    pub const EASE_IN: Self = Self::CubicBezier(0.42, 0.0, 1.0, 1.0);
    /// `ease-out`.
    pub const EASE_OUT: Self = Self::CubicBezier(0.0, 0.0, 0.58, 1.0);
    /// `ease-in-out`.
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);

    /// How far a value has moved, `t` of the way through its time: 0 at the
    /// start, 1 at the end. A bezier, or the desktop's spring, may pass
    /// either on the way; what cannot -- an opacity, a colour's channel -- is
    /// held where it is written. `t` is held to 0..=1, and one that is not a
    /// number is the end.
    #[must_use]
    pub fn ease(self, t: f32, motion: Motion) -> f32 {
        let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
        match self {
            Self::Desktop => motion.arriving(t),
            Self::Linear => t,
            Self::CubicBezier(x1, y1, x2, y2) => bezier(x1, y1, x2, y2, t),
            Self::Steps(n, position) => steps(n, position, t),
        }
    }
}

/// A cubic bezier from (0, 0) to (1, 1) through `(x1, y1)` and `(x2, y2)`,
/// read at `x`: its `y` where its `x` is `x`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    // Each coordinate a polynomial in the curve's own parameter `s`:
    // ((a s + b) s + c) s.
    let poly = |p1: f32, p2: f32| {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        (1.0 - c - b, b, c)
    };
    let (ax, bx, cx) = poly(x1, x2);
    let (ay, by, cy) = poly(y1, y2);
    let at = |(a, b, c): (f32, f32, f32), s: f32| ((a * s + b) * s + c) * s;
    // The `s` whose x is `x`: Newton's method from `x` itself, which
    // converges in a few steps for every curve CSS allows; bisection where
    // the slope is too flat for it.
    let mut s = x;
    for _ in 0..8 {
        let error = at((ax, bx, cx), s) - x;
        if error.abs() < 1e-6 {
            return at((ay, by, cy), s);
        }
        let slope = (3.0 * ax * s + 2.0 * bx) * s + cx;
        if slope.abs() < 1e-6 {
            break;
        }
        s -= error / slope;
    }
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    s = x;
    for _ in 0..32 {
        let here = at((ax, bx, cx), s);
        if (here - x).abs() < 1e-6 {
            break;
        }
        if here < x {
            low = s;
        } else {
            high = s;
        }
        s = f32::midpoint(low, high);
    }
    at((ay, by, cy), s)
}

/// `steps(n, position)` read at `t`: CSS Easing's step function.
fn steps(n: u16, position: StepPosition, t: f32) -> f32 {
    let n = f32::from(n.max(1));
    let mut step = (t * n).floor();
    if matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth) {
        step += 1.0;
    }
    let jumps = match position {
        // At least one jump: `jump-none` with a single step is refused
        // where it is read, and this keeps a hand-made one finite.
        StepPosition::JumpNone => (n - 1.0).max(1.0),
        StepPosition::JumpBoth => n + 1.0,
        StepPosition::JumpStart | StepPosition::JumpEnd => n,
    };
    step.min(jumps) / jumps
}

// ---------------------------------------------------------------------------
// What a style says moves
// ---------------------------------------------------------------------------

/// What a `transition-property` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransitionTarget {
    /// `all`: every value that can move.
    All,
    /// A property, or a shorthand's properties.
    Properties(Vec<Property>),
    /// A name this does not read: kept, as CSS keeps it, so the lists still
    /// line up -- and moving nothing.
    Unknown(String),
}

/// What a style says moves, and how: its `transition-*` lists, as given.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionSpec {
    /// What moves, in order; empty for `none`.
    pub properties: Vec<TransitionTarget>,
    /// How long each takes, in milliseconds, before the user's motion
    /// scales it.
    pub durations: Vec<f32>,
    /// How each is eased.
    pub timings: Vec<Timing>,
    /// How long each waits before it starts, in milliseconds; a negative
    /// one starts part-way.
    pub delays: Vec<f32>,
}

impl Default for TransitionSpec {
    /// CSS's initial values -- `all 0s`, which moves nothing -- with the
    /// desktop's curve for `ease`.
    fn default() -> Self {
        Self {
            properties: vec![TransitionTarget::All],
            durations: vec![0.0],
            timings: vec![Timing::Desktop],
            delays: vec![0.0],
        }
    }
}

/// How one value moves: what a [`TransitionSpec`] gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct How {
    /// Milliseconds, as stated.
    pub duration_ms: f32,
    /// The easing.
    pub timing: Timing,
    /// Milliseconds before it starts.
    pub delay_ms: f32,
}

impl TransitionSpec {
    /// Whether it could move anything: it names something, and some entry
    /// takes time or waits. One that cannot -- CSS's initial `all 0s` --
    /// changes every value at once.
    #[must_use]
    pub fn moves_anything(&self) -> bool {
        !self.properties.is_empty()
            && (self.durations.iter().any(|&d| d > 0.0) || self.delays.iter().any(|&d| d > 0.0))
    }

    /// How `what` moves: the last entry that names it, or `all`, with the
    /// duration, timing and delay at its place in their lists -- each
    /// repeated as often as it takes to be as long as the properties', as
    /// CSS repeats them. `None` if no entry names it.
    #[must_use]
    pub fn how(&self, what: Animated) -> Option<How> {
        let at = self.properties.iter().rposition(|target| match target {
            TransitionTarget::All => true,
            TransitionTarget::Properties(ps) => ps.iter().any(|&p| Animated::of(p) == Some(what)),
            TransitionTarget::Unknown(_) => false,
        })?;
        let pick = |len: usize| at.checked_rem(len);
        Some(How {
            duration_ms: pick(self.durations.len())
                .and_then(|i| self.durations.get(i))
                .copied()
                .unwrap_or(0.0),
            timing: pick(self.timings.len())
                .and_then(|i| self.timings.get(i))
                .copied()
                .unwrap_or(Timing::Desktop),
            delay_ms: pick(self.delays.len())
                .and_then(|i| self.delays.get(i))
                .copied()
                .unwrap_or(0.0),
        })
    }
}

// ---------------------------------------------------------------------------
// What moves
// ---------------------------------------------------------------------------

/// One of a style's values a transition can move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Animated {
    /// The text's colour.
    Color,
    /// The background.
    Background,
    /// A side's border colour.
    BorderColor(Side),
    /// A side's margin colour.
    MarginColor(Side),
    /// A side's border width.
    BorderWidth(Side),
    /// A corner's radius.
    Radius(Corner),
    /// The opacity.
    Opacity,
    /// The font's size.
    FontSize,
    /// The line height.
    LineHeight,
    /// The box's shadow.
    Shadow,
    /// The text's shadow.
    TextShadow,
    /// A side's padding.
    Padding(Side),
    /// A side's margin.
    Margin(Side),
    /// The width.
    Width,
    /// The height.
    Height,
    /// The least width.
    MinWidth,
    /// The most width.
    MaxWidth,
    /// The least height.
    MinHeight,
    /// The most height.
    MaxHeight,
    /// A side's inset: a positioned widget's distance from its container's
    /// edge.
    Inset(Side),
}

impl Animated {
    /// What `property` moves, if it can be part-way.
    #[must_use]
    pub const fn of(property: Property) -> Option<Self> {
        Some(match property {
            Property::Color => Self::Color,
            Property::BackgroundColor => Self::Background,
            Property::BorderColor(side) => Self::BorderColor(side),
            Property::MarginColor(side) => Self::MarginColor(side),
            Property::BorderWidth(side) => Self::BorderWidth(side),
            Property::BorderRadius(corner) => Self::Radius(corner),
            Property::Opacity => Self::Opacity,
            Property::FontSize => Self::FontSize,
            Property::LineHeight => Self::LineHeight,
            Property::BoxShadow => Self::Shadow,
            Property::TextShadow => Self::TextShadow,
            Property::Padding(side) => Self::Padding(side),
            Property::Margin(side) => Self::Margin(side),
            Property::Width => Self::Width,
            Property::Height => Self::Height,
            Property::MinWidth => Self::MinWidth,
            Property::MaxWidth => Self::MaxWidth,
            Property::MinHeight => Self::MinHeight,
            Property::MaxHeight => Self::MaxHeight,
            Property::Inset(side) => Self::Inset(side),
            Property::FontFamily
            | Property::FontWeight
            | Property::TextAlign
            | Property::BorderStyle(_)
            | Property::Cursor
            | Property::TransitionProperty
            | Property::TransitionDuration
            | Property::TransitionTimingFunction
            | Property::TransitionDelay
            | Property::Position
            | Property::ZIndex => return None,
        })
    }

    /// Every value that can move: what `all` names.
    pub fn all() -> impl Iterator<Item = Self> {
        let sides = Side::ALL.into_iter().flat_map(|s| {
            [
                Self::BorderColor(s),
                Self::MarginColor(s),
                Self::BorderWidth(s),
                Self::Padding(s),
                Self::Margin(s),
                Self::Inset(s),
            ]
        });
        [
            Self::Color,
            Self::Background,
            Self::Opacity,
            Self::FontSize,
            Self::LineHeight,
            Self::Shadow,
            Self::TextShadow,
            Self::Width,
            Self::Height,
            Self::MinWidth,
            Self::MaxWidth,
            Self::MinHeight,
            Self::MaxHeight,
        ]
        .into_iter()
        .chain(sides)
        .chain(Corner::ALL.into_iter().map(Self::Radius))
    }

    /// Whether `lengths` give it as a percentage of the container: known
    /// only where the widget is laid out ([`Transitions::settle`]).
    #[must_use]
    pub fn waits_on_container(self, lengths: &BoxLengths) -> bool {
        let percent = |l: Option<super::value::Length>| l.is_some_and(|l| l.has_percent());
        let side = |edges: &[Option<super::value::Length>; 4], s: Side| {
            percent(edges.get(side_index(s)).copied().flatten())
        };
        match self {
            Self::Width => percent(lengths.width.flatten()),
            Self::Height => percent(lengths.height.flatten()),
            Self::MinWidth => percent(lengths.min_width),
            Self::MaxWidth => percent(lengths.max_width.flatten()),
            Self::MinHeight => percent(lengths.min_height),
            Self::MaxHeight => percent(lengths.max_height.flatten()),
            Self::Padding(s) => side(&lengths.padding, s),
            Self::Margin(s) => side(&lengths.margin, s),
            Self::Inset(s) => percent(
                lengths
                    .inset
                    .get(side_index(s))
                    .copied()
                    .flatten()
                    .flatten(),
            ),
            // Of the box's own size, settled where it is laid out
            // ([`Transitions::settle_radii`]).
            Self::Radius(c) => lengths
                .radius
                .get(super::compute::corner_index(c))
                .is_some_and(Option::is_some),
            _ => false,
        }
    }

    /// Whether `lengths` give it -- a box's length a style set, which
    /// [`BoxLengths::apply`] writes when the container settles them.
    #[must_use]
    pub fn set_in(self, lengths: &BoxLengths) -> bool {
        let side = |edges: &[Option<super::value::Length>; 4], s: Side| {
            edges.get(side_index(s)).is_some_and(Option::is_some)
        };
        match self {
            Self::Width => lengths.width.is_some(),
            Self::Height => lengths.height.is_some(),
            Self::MinWidth => lengths.min_width.is_some(),
            Self::MaxWidth => lengths.max_width.is_some(),
            Self::MinHeight => lengths.min_height.is_some(),
            Self::MaxHeight => lengths.max_height.is_some(),
            Self::Padding(s) => side(&lengths.padding, s),
            Self::Margin(s) => side(&lengths.margin, s),
            Self::Inset(s) => lengths
                .inset
                .get(side_index(s))
                .is_some_and(Option::is_some),
            _ => false,
        }
    }

    /// Its value in `style`.
    fn read(self, style: &Style) -> Part {
        let edge = |e: &crate::style::Edges, s: Side| match s {
            Side::Top => e.top,
            Side::Right => e.right,
            Side::Bottom => e.bottom,
            Side::Left => e.left,
        };
        let border = |s: Side| match s {
            Side::Top => style.border.top,
            Side::Right => style.border.right,
            Side::Bottom => style.border.bottom,
            Side::Left => style.border.left,
        };
        match self {
            Self::Color => Part::MaybeColor(style.foreground),
            Self::Background => Part::Color(style.background),
            Self::BorderColor(s) => Part::Color(border(s).color),
            Self::MarginColor(s) => Part::Color(match s {
                Side::Top => style.margin_color.top,
                Side::Right => style.margin_color.right,
                Side::Bottom => style.margin_color.bottom,
                Side::Left => style.margin_color.left,
            }),
            Self::BorderWidth(s) => Part::Number(border(s).width),
            Self::Radius(c) => Part::Number(match c {
                Corner::TopLeft => style.border_radius.top_left,
                Corner::TopRight => style.border_radius.top_right,
                Corner::BottomRight => style.border_radius.bottom_right,
                Corner::BottomLeft => style.border_radius.bottom_left,
            }),
            Self::Opacity => Part::Number(style.opacity),
            Self::FontSize => Part::Number(style.font_size),
            Self::LineHeight => Part::Number(style.line_height),
            Self::Shadow => Part::Shadow(style.shadow),
            Self::TextShadow => Part::Shadow(style.text_shadow),
            Self::Padding(s) => Part::Number(edge(&style.padding, s)),
            Self::Margin(s) => Part::Number(edge(&style.margin, s)),
            Self::Width => Part::MaybeNumber(style.width),
            Self::Height => Part::MaybeNumber(style.height),
            Self::MinWidth => Part::MaybeNumber(style.min_width),
            Self::MaxWidth => Part::MaybeNumber(style.max_width),
            Self::MinHeight => Part::MaybeNumber(style.min_height),
            Self::MaxHeight => Part::MaybeNumber(style.max_height),
            Self::Inset(s) => Part::MaybeNumber(match s {
                Side::Top => style.inset.top,
                Side::Right => style.inset.right,
                Side::Bottom => style.inset.bottom,
                Side::Left => style.inset.left,
            }),
        }
    }

    /// Write `part` into `style` as its value -- held where the value cannot
    /// go: no length but a margin below nought, no opacity past 0 to 1. A
    /// part of another kind than its own changes nothing.
    fn write(self, style: &mut Style, part: Part) {
        let edge = |e: &mut crate::style::Edges, s: Side, v: f32| match s {
            Side::Top => e.top = v,
            Side::Right => e.right = v,
            Side::Bottom => e.bottom = v,
            Side::Left => e.left = v,
        };
        let border = border_of;
        let least = |v: f32| v.max(0.0);
        match (self, part) {
            (Self::Color, Part::MaybeColor(c)) => style.foreground = c,
            (Self::Background, Part::Color(c)) => style.background = c,
            (Self::BorderColor(s), Part::Color(c)) => border(style, s).color = c,
            (Self::MarginColor(s), Part::Color(c)) => match s {
                Side::Top => style.margin_color.top = c,
                Side::Right => style.margin_color.right = c,
                Side::Bottom => style.margin_color.bottom = c,
                Side::Left => style.margin_color.left = c,
            },
            (Self::BorderWidth(s), Part::Number(v)) => border(style, s).width = least(v),
            (Self::Radius(c), Part::Number(v)) => {
                let r = &mut style.border_radius;
                let v = least(v);
                match c {
                    Corner::TopLeft => r.top_left = v,
                    Corner::TopRight => r.top_right = v,
                    Corner::BottomRight => r.bottom_right = v,
                    Corner::BottomLeft => r.bottom_left = v,
                }
            }
            (Self::Opacity, Part::Number(v)) => style.opacity = v.clamp(0.0, 1.0),
            (Self::FontSize, Part::Number(v)) => style.font_size = least(v),
            (Self::LineHeight, Part::Number(v)) => style.line_height = least(v),
            (Self::Shadow, Part::Shadow(s)) => style.shadow = s,
            (Self::TextShadow, Part::Shadow(s)) => style.text_shadow = s,
            (Self::Padding(s), Part::Number(v)) => edge(&mut style.padding, s, least(v)),
            (Self::Margin(s), Part::Number(v)) => edge(&mut style.margin, s, v),
            (Self::Width, Part::MaybeNumber(v)) => style.width = v.map(least),
            (Self::Height, Part::MaybeNumber(v)) => style.height = v.map(least),
            (Self::MinWidth, Part::MaybeNumber(v)) => style.min_width = v.map(least),
            (Self::MaxWidth, Part::MaybeNumber(v)) => style.max_width = v.map(least),
            (Self::MinHeight, Part::MaybeNumber(v)) => style.min_height = v.map(least),
            (Self::MaxHeight, Part::MaybeNumber(v)) => style.max_height = v.map(least),
            // An inset may be negative.
            (Self::Inset(s), Part::MaybeNumber(v)) => match s {
                Side::Top => style.inset.top = v,
                Side::Right => style.inset.right = v,
                Side::Bottom => style.inset.bottom = v,
                Side::Left => style.inset.left = v,
            },
            _ => {}
        }
    }
}

/// `side`'s place in a box's per-side lists: top, right, bottom, left.
const fn side_index(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

/// `style`'s border on `side`.
const fn border_of(style: &mut Style, side: Side) -> &mut crate::style::Border {
    match side {
        Side::Top => &mut style.border.top,
        Side::Right => &mut style.border.right,
        Side::Bottom => &mut style.border.bottom,
        Side::Left => &mut style.border.left,
    }
}

/// A value of a style, as a transition moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// A colour.
    Color(Color),
    /// A colour that may be the theme's (`None`), which has no number here.
    MaybeColor(Option<Color>),
    /// A number.
    Number(f32),
    /// A number that may be `auto` or `none` (`None`).
    MaybeNumber(Option<f32>),
    /// A shadow, or none.
    Shadow(Option<BoxShadow>),
}

impl Part {
    /// The value `p` of the way from `self` to `to`, or `None` where there
    /// is no way between them -- a size that was `auto`, a colour the
    /// theme chose -- and the value changes at once. `p` may pass 0 or 1.
    fn toward(self, to: Self, p: f32) -> Option<Self> {
        let number = |a: f32, b: f32| a + (b - a) * p;
        match (self, to) {
            (Self::Color(a), Self::Color(b)) => Some(Self::Color(mix(a, b, p))),
            (Self::MaybeColor(Some(a)), Self::MaybeColor(Some(b))) => {
                Some(Self::MaybeColor(Some(mix(a, b, p))))
            }
            (Self::Number(a), Self::Number(b)) => Some(Self::Number(number(a, b))),
            (Self::MaybeNumber(Some(a)), Self::MaybeNumber(Some(b))) => {
                Some(Self::MaybeNumber(Some(number(a, b))))
            }
            // A shadow from none, or to none, is CSS's: from a transparent
            // one in the same place.
            (Self::Shadow(a), Self::Shadow(b)) => {
                let (a, b) = match (a, b) {
                    (None, None) => return None,
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, faint(a)),
                    (None, Some(b)) => (faint(b), b),
                };
                Some(Self::Shadow(Some(BoxShadow {
                    offset_x: number(a.offset_x, b.offset_x),
                    offset_y: number(a.offset_y, b.offset_y),
                    blur: number(a.blur, b.blur).max(0.0),
                    spread: number(a.spread, b.spread),
                    color: mix(a.color, b.color, p),
                })))
            }
            _ => None,
        }
    }
}

/// `shadow`, transparent: where a shadow from none starts.
const fn faint(shadow: BoxShadow) -> BoxShadow {
    BoxShadow {
        color: Color::rgba(shadow.color.r, shadow.color.g, shadow.color.b, 0),
        ..shadow
    }
}

/// The colour `p` of the way from `a` to `b`, mixed as CSS mixes a
/// transition's: each channel weighted by its alpha, so a colour fading in
/// from transparent does not pass through the transparent one's black.
/// Every channel is held to 0..=255 where `p` passes 0 or 1.
fn mix(a: Color, b: Color, p: f32) -> Color {
    let channel = |v: u8| f32::from(v);
    let lerp = |x: f32, y: f32| x + (y - x) * p;
    let (aa, ba) = (channel(a.a) / 255.0, channel(b.a) / 255.0);
    let alpha = lerp(aa, ba).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return Color::rgba(0, 0, 0, 0);
    }
    let blend = |x: u8, y: u8| to_channel(lerp(channel(x) * aa, channel(y) * ba) / alpha);
    Color::rgba(
        blend(a.r, b.r),
        blend(a.g, b.g),
        blend(a.b, b.b),
        to_channel(alpha * 255.0),
    )
}

/// A channel's value, rounded and held to 0..=255.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "held to 0..=255 and rounded first, so the cast is exact"
)]
fn to_channel(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    v.round().clamp(0.0, 255.0) as u8
}

// ---------------------------------------------------------------------------
// A widget's transitions
// ---------------------------------------------------------------------------

/// One value moving.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Running {
    /// What moves.
    what: Animated,
    /// Where it started: what was shown when its value changed.
    from: Part,
    /// Where it is going: the value its style now gives.
    to: Part,
    /// When it starts moving, by the tree's clock: the change, and its
    /// delay -- earlier than the change for a negative one.
    start_ms: f64,
    /// How long it moves for, the user's motion and any reversal applied.
    duration_ms: f64,
    /// Its easing.
    timing: Timing,
    /// CSS's "reversing-adjusted start value": where the transition it
    /// reversed was going from -- what a change back toward it is a
    /// reversal of. Its own start where it reversed nothing.
    reversing_from: Part,
    /// CSS's "reversing shortening factor": how much of its full length it
    /// takes -- 1 where it reversed nothing, less for a way back from
    /// part-way.
    shortening: f32,
}

impl Running {
    /// How far through its time it is at `now`, 0 to 1 -- 0 during its
    /// delay -- or `None` once it is over.
    fn progress(&self, now: f64) -> Option<f32> {
        let end = self.start_ms + self.duration_ms;
        if now >= end {
            return None;
        }
        if now <= self.start_ms || self.duration_ms <= 0.0 {
            return Some(0.0);
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a fraction from 0 to 1, which f32 holds as well as any value this moves"
        )]
        let t = ((now - self.start_ms) / self.duration_ms) as f32;
        Some(t)
    }

    /// Its value at `now` under `motion`, or `None` once it is over.
    fn at(&self, now: f64, motion: Motion) -> Option<Part> {
        let t = self.progress(now)?;
        let eased = self.timing.ease(t, motion);
        // Mixable: it was checked when it started.
        self.from.toward(self.to, eased)
    }

    /// How far it has eased at `now`: what a reversal of it is shortened by.
    fn eased(&self, now: f64, motion: Motion) -> f32 {
        self.progress(now)
            .map_or(1.0, |t| self.timing.ease(t, motion))
    }
}

/// A widget's transitions: what its style last was, and what of it is
/// moving. Kept by the widget from one layout to the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transitions {
    /// Each value as its style last gave it: what a change is a change
    /// from. Empty before its first style, which changes nothing.
    targets: BTreeMap<Animated, Part>,
    /// What is moving.
    running: Vec<Running>,
    /// The tree's clock when its style was last computed, and the motion
    /// then: what [`settle`](Self::settle) moves by.
    now_ms: f64,
    /// See `now_ms`.
    motion: Option<Motion>,
}

impl Transitions {
    /// A widget's transitions from now on, its style until now `shown` --
    /// what a change is a change from. Nothing moves yet.
    #[must_use]
    pub fn from_shown(shown: &Style) -> Self {
        Self {
            targets: Animated::all().map(|a| (a, a.read(shown))).collect(),
            ..Self::default()
        }
    }

    /// Whether anything is moving.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        !self.running.is_empty()
    }

    /// Whether `what` is moving.
    #[must_use]
    pub fn moves(&self, what: Animated) -> bool {
        self.running.iter().any(|r| r.what == what)
    }

    /// Take up a widget's style as just computed, at `now` by the tree's
    /// clock under the user's `motion`: `shown` is that style, and leaves
    /// with each moving value where it is now. A value that differs from
    /// the one last computed starts moving if `spec` -- the new style's
    /// `transition` -- says it does; a box's length that waits on its
    /// container (`lengths`) is left for [`settle`](Self::settle).
    pub fn update(
        &mut self,
        shown: &mut Style,
        spec: &TransitionSpec,
        lengths: &BoxLengths,
        now: f64,
        motion: Motion,
    ) {
        self.now_ms = now;
        self.motion = Some(motion);
        let target = shown.clone();
        for what in Animated::all().filter(|a| !a.waits_on_container(lengths)) {
            self.take_up(what, &target, shown, spec);
        }
    }

    /// Take up a box's lengths now that its container has settled `lengths`
    /// into `shown` ([`BoxLengths::apply`]), at the time of the last
    /// [`update`](Self::update): those that waited on it may start moving,
    /// and each the settling wrote over is put back where it is now. A
    /// length no style set is not the settling's, and was taken up then.
    pub fn settle(&mut self, shown: &mut Style, spec: &TransitionSpec, lengths: &BoxLengths) {
        let target = shown.clone();
        for what in Animated::all().filter(|a| a.set_in(lengths)) {
            self.take_up(what, &target, shown, spec);
        }
    }

    /// Take up the corners' radii that are percentages of the box's own
    /// size, now that it is laid out and [`BoxLengths::apply_radii`] has
    /// settled them into `shown`, as [`settle`](Self::settle) takes up a
    /// container's percentages.
    pub fn settle_radii(&mut self, shown: &mut Style, spec: &TransitionSpec, lengths: &BoxLengths) {
        let target = shown.clone();
        for corner in Corner::ALL {
            if lengths
                .radius
                .get(super::compute::corner_index(corner))
                .is_some_and(Option::is_some)
            {
                self.take_up(Animated::Radius(corner), &target, shown, spec);
            }
        }
    }

    /// Take up `what`'s value in `target`, and write where it is now into
    /// `shown`.
    fn take_up(
        &mut self,
        what: Animated,
        target: &Style,
        shown: &mut Style,
        spec: &TransitionSpec,
    ) {
        let now = self.now_ms;
        let Some(motion) = self.motion else {
            return;
        };
        let to = what.read(target);
        let before = self.targets.insert(what, to);
        let running = self.running.iter().position(|r| r.what == what);
        // Nothing moves with animations off; and, as in CSS, nothing keeps
        // moving that the style now says nothing about.
        if motion.is_still() || spec.how(what).is_none() {
            if let Some(i) = running {
                self.running.swap_remove(i);
            }
            return;
        }
        if let Some(previous) = before
            && previous != to
        {
            let old = running.map(|i| self.running.swap_remove(i));
            // What is shown now, before the change: part-way, if it was
            // moving.
            let current = old.and_then(|r| r.at(now, motion)).unwrap_or(previous);
            if let Some(started) = start(what, current, to, old, spec, now, motion) {
                self.running.push(started);
            }
        }
        // Where it is now; over, it is its style's.
        if let Some(i) = self.running.iter().position(|r| r.what == what) {
            match self.running.get(i).and_then(|r| r.at(now, motion)) {
                Some(part) => what.write(shown, part),
                None => {
                    self.running.swap_remove(i);
                }
            }
        }
    }
}

/// The transition a change of `what` from `current` to `to` starts at
/// `now`, if `spec` moves it and there is a way between the two -- after
/// `old`, the one it was on, if any.
fn start(
    what: Animated,
    current: Part,
    to: Part,
    old: Option<Running>,
    spec: &TransitionSpec,
    now: f64,
    motion: Motion,
) -> Option<Running> {
    let how = spec.how(what)?;
    if current == to || current.toward(to, 0.5).is_none() {
        return None;
    }
    // CSS's reversal: back toward where the old one started, it takes as
    // much of its length as the way back is.
    let (reversing_from, shortening) = match old {
        Some(old) if old.reversing_from == to => {
            let factor = old.eased(now, motion) * old.shortening + 1.0 - old.shortening;
            (old.to, factor.abs().clamp(0.0, 1.0))
        }
        _ => (current, 1.0),
    };
    let stated = how.duration_ms.max(0.0);
    let duration = f64::from(scaled(stated, motion)) * f64::from(shortening);
    let delay = f64::from(how.delay_ms);
    let delay = if delay < 0.0 {
        delay * f64::from(shortening)
    } else {
        delay
    };
    // CSS starts none whose time, delay and all, comes to nothing.
    if duration + delay <= 0.0 {
        return None;
    }
    Some(Running {
        what,
        from: current,
        to,
        start_ms: now + delay,
        duration_ms: duration.max(0.0),
        timing: how.timing,
        reversing_from,
        shortening,
    })
}

/// `stated` milliseconds under the user's `motion`: scaled with the
/// desktop's transitions, stated against its standard.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a non-negative duration, rounded and held to u32's range before the cast; \
              u32::MAX as f32 rounds up, and the cast saturates"
)]
fn scaled(stated: f32, motion: Motion) -> u32 {
    let whole = stated.round().clamp(0.0, u32::MAX as f32) as u32;
    motion.duration_ms(whole)
}

#[cfg(test)]
#[path = "transition_tests.rs"]
mod tests;
