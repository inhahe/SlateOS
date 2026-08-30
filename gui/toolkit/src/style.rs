//! Styling system for the GUI toolkit.
//!
//! Provides a simple, purpose-built styling language for widgets.
//! Avoids CSS complexity (cascade, specificity) while keeping useful
//! properties: colors, borders, padding, margins, fonts, border-radius.

use crate::color::Color;

/// Edge insets (padding, margins, borders).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// Uniform edges on all sides.
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Symmetric edges (vertical, horizontal).
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Total horizontal space.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Total vertical space.
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Border style for one edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            width: 0.0,
            color: Color::BLACK,
        }
    }
}

/// Four-sided border.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Borders {
    pub top: Border,
    pub right: Border,
    pub bottom: Border,
    pub left: Border,
}

impl Borders {
    /// Uniform border on all sides.
    pub fn all(width: f32, color: Color) -> Self {
        let b = Border { width, color };
        Self {
            top: b,
            right: b,
            bottom: b,
            left: b,
        }
    }
}

/// A drop shadow, apart from the shape that casts it.
///
/// One value rather than five loose parameters because the five are one
/// decision: an offset without the blur that goes with it is a different
/// shadow, not a partly-specified one. It also lets a surface style name its
/// shadow once and reuse it at every place it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset_x: f32,
    pub offset_y: f32,
    /// How far the edge fades out over.
    pub blur: f32,
    /// How much larger than the caster the shadow is drawn, before blurring.
    pub spread: f32,
    /// Usually black at low alpha: a shadow is the absence of light, not a
    /// colour of its own. A grey one reads as a drawn border over a light
    /// background and is brighter than what it falls on over a dark one.
    pub color: Color,
}

impl Shadow {
    /// A shadow cast straight down, with no spread — what a surface floating a
    /// short way above its background casts.
    #[must_use]
    pub const fn drop(offset_y: f32, blur: f32, color: Color) -> Self {
        Self {
            offset_x: 0.0,
            offset_y,
            blur,
            spread: 0.0,
            color,
        }
    }
}

/// Corner radii for rounded rectangles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub const ZERO: Self = Self {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };

    /// Uniform radius on all corners.
    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// Rounded across the top, square across the bottom.
    ///
    /// The shape of anything that sits on top of a larger square surface and
    /// shares its lower edge — a window title bar above the client area, a tab
    /// above its page. Rounding the bottom corners too would cut a notch out of
    /// the join between the two.
    pub const fn top(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: 0.0,
            bottom_left: 0.0,
        }
    }
}

/// Text alignment within a widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Font weight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontWeight {
    Thin,
    Light,
    #[default]
    Regular,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
}

/// Complete style for a widget.
///
/// Styles are applied directly to widgets (no cascade/inheritance).
/// Each widget has its own complete style. Themes provide defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    // Background
    pub background: Color,

    // Foreground (text color)
    pub foreground: Color,

    // Padding (inside border)
    pub padding: Edges,

    // Margin (outside border)
    pub margin: Edges,

    // Border
    pub border: Borders,
    pub border_radius: CornerRadii,

    // Typography
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub text_align: TextAlign,
    pub line_height: f32,

    // Size constraints
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,

    // Opacity
    pub opacity: f32,

    // Cursor style
    pub cursor: Cursor,

    // Box shadow (simplified: single shadow)
    pub shadow: Option<BoxShadow>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            foreground: Color::BLACK,
            padding: Edges::ZERO,
            margin: Edges::ZERO,
            border: Borders::default(),
            border_radius: CornerRadii::ZERO,
            font_size: 14.0,
            font_weight: FontWeight::Regular,
            text_align: TextAlign::Left,
            line_height: 1.4,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            opacity: 1.0,
            cursor: Cursor::Default,
            shadow: None,
        }
    }
}

/// Mouse cursor style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    Text,
    Crosshair,
    Move,
    ResizeNS,
    ResizeEW,
    ResizeNESW,
    ResizeNWSE,
    NotAllowed,
    Wait,
}

/// Box shadow parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}
