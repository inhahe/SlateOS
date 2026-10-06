//! Declarations: the properties this reads, and what each may be given --
//! a shorthand read into the properties it sets.
//!
//! A declaration whose value names a variable (`var(--accent)`) cannot be
//! read until the variables are known, which is where the widget is: it is
//! kept as written ([`Declared::Pending`]) and read by [`read`] then. A
//! custom property (`--gap: 4px`) is kept as written too, for `var()` to
//! substitute ([`Declared::Custom`]).

use super::token::{Spanned, Token};
use super::transition::{StepPosition, Timing, TransitionTarget};
use super::value::{self, ColorValue, Cursor, Length, ValueError};
use crate::style::{Cursor as PointerCursor, TextAlign};

/// One side of a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    /// The top.
    Top,
    /// The right.
    Right,
    /// The bottom.
    Bottom,
    /// The left.
    Left,
}

impl Side {
    /// Every side, in CSS's order: top, right, bottom, left.
    pub const ALL: [Self; 4] = [Self::Top, Self::Right, Self::Bottom, Self::Left];

    fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
        }
    }
}

/// One corner of a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Corner {
    /// The top left.
    TopLeft,
    /// The top right.
    TopRight,
    /// The bottom right.
    BottomRight,
    /// The bottom left.
    BottomLeft,
}

impl Corner {
    /// Every corner, in CSS's order: from the top left, clockwise.
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomRight,
        Self::BottomLeft,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomRight => "bottom-right",
            Self::BottomLeft => "bottom-left",
        }
    }
}

/// A property this reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Property {
    /// `color`: the text's.
    Color,
    /// `background-color`.
    BackgroundColor,
    /// `font-family`.
    FontFamily,
    /// `font-size`.
    FontSize,
    /// `font-weight`.
    FontWeight,
    /// `line-height`.
    LineHeight,
    /// `text-align`.
    TextAlign,
    /// `margin-*`.
    Margin(Side),
    /// `padding-*`.
    Padding(Side),
    /// `border-*-width`.
    BorderWidth(Side),
    /// `border-*-color`.
    BorderColor(Side),
    /// `border-*-style`.
    BorderStyle(Side),
    /// `border-*-radius`.
    BorderRadius(Corner),
    /// `width`.
    Width,
    /// `height`.
    Height,
    /// `min-width`.
    MinWidth,
    /// `max-width`.
    MaxWidth,
    /// `min-height`.
    MinHeight,
    /// `max-height`.
    MaxHeight,
    /// `opacity`.
    Opacity,
    /// `box-shadow`.
    BoxShadow,
    /// `text-shadow`.
    TextShadow,
    /// `cursor`: the pointer's shape over it.
    Cursor,
    /// `transition-property`: what moves when it changes.
    TransitionProperty,
    /// `transition-duration`: how long each takes.
    TransitionDuration,
    /// `transition-timing-function`: how each is eased.
    TransitionTimingFunction,
    /// `transition-delay`: how long each waits.
    TransitionDelay,
}

impl Property {
    /// Whether a widget takes this from its parent when it does not set it:
    /// CSS's inherited properties among these.
    #[must_use]
    pub const fn inherited(self) -> bool {
        matches!(
            self,
            Self::Color
                | Self::FontFamily
                | Self::FontSize
                | Self::FontWeight
                | Self::LineHeight
                | Self::TextAlign
                | Self::TextShadow
                | Self::Cursor
        )
    }
}

/// A family a font is asked for in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Family {
    /// The interface's face: `system-ui`, `sans-serif`, `serif`, `cursive`,
    /// `fantasy` -- the user's choice of face for the desktop's text.
    Ui,
    /// The fixed-pitch face: `monospace`, `ui-monospace`.
    Mono,
    /// A family by its name.
    Named(String),
}

/// A border's style. Only whether there is one is drawn: dotted, dashed and
/// the rest are drawn solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    /// No border: `none`, `hidden`.
    None,
    /// A border.
    Solid,
}

/// A shadow: a box's, or text's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowValue {
    /// Across.
    pub x: Length,
    /// Down.
    pub y: Length,
    /// How far it blurs.
    pub blur: Length,
    /// How far it grows past the box before blurring (a box's only).
    pub spread: Length,
    /// Its colour: the text's unless it says.
    pub color: ColorValue,
}

/// What a line's height is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// The font's own: `normal`.
    Normal,
    /// This many times the font's size.
    Multiple(f32),
    /// This tall.
    Length(Length),
}

/// What a property is given.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A colour.
    Color(ColorValue),
    /// A length.
    Length(Length),
    /// `auto`: the size its content and its container give it.
    Auto,
    /// `none`: no limit, no shadow.
    None,
    /// A number: an opacity.
    Number(f32),
    /// Families, the first one installed used.
    FontFamily(Vec<Family>),
    /// A weight from 1 to 1000; `bolder` and `lighter` are relative to the
    /// parent's and come as [`Value::Bolder`] and [`Value::Lighter`].
    FontWeight(u16),
    /// A step heavier than the parent's weight.
    Bolder,
    /// A step lighter than the parent's weight.
    Lighter,
    /// A line's height.
    LineHeight(LineHeight),
    /// A text alignment.
    TextAlign(TextAlign),
    /// A border's style.
    BorderStyle(BorderStyle),
    /// A shadow.
    Shadow(ShadowValue),
    /// A pointer's shape.
    Cursor(PointerCursor),
    /// What moves when it changes: `none` is an empty list.
    Transitions(Vec<TransitionTarget>),
    /// Times, in milliseconds: durations or delays.
    Times(Vec<f32>),
    /// Timing functions.
    Timings(Vec<Timing>),
    /// `inherit`: the parent's value.
    Inherit,
    /// `initial`: the property's own default -- the toolkit's.
    Initial,
}

/// A declaration, as kept.
#[derive(Clone, Debug, PartialEq)]
pub enum Declared {
    /// A property, read.
    Value(Property, Value),
    /// A declaration whose value names a variable: its property's name as
    /// written and its tokens, read where the variables are known.
    Pending {
        /// The property as written, lower-cased.
        name: String,
        /// Its value.
        tokens: Vec<Spanned>,
    },
    /// A custom property: its name, `--` included, and its value.
    Custom {
        /// `--name`.
        name: String,
        /// Its value, as written.
        tokens: Vec<Spanned>,
    },
}

/// Whether `tokens` name a variable anywhere.
#[must_use]
pub fn uses_var(tokens: &[Spanned]) -> bool {
    tokens
        .iter()
        .any(|s| matches!(&s.token, Token::Function(f) if f.eq_ignore_ascii_case("var")))
}

/// The declaration `name: tokens`, kept: read now unless it waits on a
/// variable.
///
/// # Errors
///
/// Why it cannot be read.
pub fn declare(name: &str, tokens: &[Spanned]) -> Result<Vec<Declared>, ValueError> {
    if name.starts_with("--") {
        return Ok(vec![Declared::Custom {
            name: name.to_string(),
            tokens: trim(tokens).to_vec(),
        }]);
    }
    let lower = name.to_ascii_lowercase();
    if uses_var(tokens) {
        // Still has to be a property this reads: said now, not when drawn.
        if !is_known(&lower) {
            return Err(unknown(&lower));
        }
        return Ok(vec![Declared::Pending {
            name: lower,
            tokens: trim(tokens).to_vec(),
        }]);
    }
    Ok(read(&lower, tokens)?
        .into_iter()
        .map(|(p, v)| Declared::Value(p, v))
        .collect())
}

/// `tokens` without the whitespace at either end.
fn trim(tokens: &[Spanned]) -> &[Spanned] {
    let start = tokens
        .iter()
        .position(|s| s.token != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|s| s.token != Token::Whitespace)
        .map_or(start, |i| i.saturating_add(1));
    tokens.get(start..end).unwrap_or(&[])
}

/// What is said of a property this does not read.
fn unknown(name: &str) -> String {
    match name {
        "position" | "z-index" | "top" | "left" | "right" | "bottom" => {
            format!("`{name}` is not read yet")
        }
        _ => format!("`{name}` is not a property this reads"),
    }
}

/// Whether `name` -- lower-cased -- is a property or shorthand this reads.
fn is_known(name: &str) -> bool {
    properties_of(name).is_some()
}

/// The longhand property `name` -- lower-cased.
fn longhand(name: &str) -> Option<Property> {
    let simple = match name {
        "color" => Some(Property::Color),
        "background-color" => Some(Property::BackgroundColor),
        "font-family" => Some(Property::FontFamily),
        "font-size" => Some(Property::FontSize),
        "font-weight" => Some(Property::FontWeight),
        "line-height" => Some(Property::LineHeight),
        "text-align" => Some(Property::TextAlign),
        "width" => Some(Property::Width),
        "height" => Some(Property::Height),
        "min-width" => Some(Property::MinWidth),
        "max-width" => Some(Property::MaxWidth),
        "min-height" => Some(Property::MinHeight),
        "max-height" => Some(Property::MaxHeight),
        "opacity" => Some(Property::Opacity),
        "box-shadow" => Some(Property::BoxShadow),
        "text-shadow" => Some(Property::TextShadow),
        "cursor" => Some(Property::Cursor),
        "transition-property" => Some(Property::TransitionProperty),
        "transition-duration" => Some(Property::TransitionDuration),
        "transition-timing-function" => Some(Property::TransitionTimingFunction),
        "transition-delay" => Some(Property::TransitionDelay),
        _ => None,
    };
    if simple.is_some() {
        return simple;
    }
    for side in Side::ALL {
        let s = side.name();
        if name == format!("margin-{s}") {
            return Some(Property::Margin(side));
        }
        if name == format!("padding-{s}") {
            return Some(Property::Padding(side));
        }
        if name == format!("border-{s}-width") {
            return Some(Property::BorderWidth(side));
        }
        if name == format!("border-{s}-color") {
            return Some(Property::BorderColor(side));
        }
        if name == format!("border-{s}-style") {
            return Some(Property::BorderStyle(side));
        }
    }
    Corner::ALL
        .into_iter()
        .find(|c| name == format!("border-{}-radius", c.name()))
        .map(Property::BorderRadius)
}

/// The declaration `name: tokens` -- lower-cased, with no `var()` left in
/// it -- read into the properties it sets: one for a longhand, each of a
/// shorthand's.
///
/// # Errors
///
/// Why it cannot be read: a property this does not read, or a value it does
/// not take.
pub fn read(name: &str, tokens: &[Spanned]) -> Result<Vec<(Property, Value)>, ValueError> {
    let tokens = trim(tokens);
    if tokens.is_empty() {
        return Err(format!("`{name}` is given nothing"));
    }
    // The keywords every property takes, and a shorthand passes to each of
    // its properties.
    if let [
        Spanned {
            token: Token::Ident(word),
            ..
        },
    ] = tokens
    {
        let keyword = match word.to_ascii_lowercase().as_str() {
            "inherit" => Some(Value::Inherit),
            "initial" => Some(Value::Initial),
            _ => None,
        };
        if let Some(keyword) = keyword {
            let props = properties_of(name).ok_or_else(|| unknown(name))?;
            return Ok(props.into_iter().map(|p| (p, keyword.clone())).collect());
        }
    }
    let mut c = Cursor::new(tokens);
    let out = match name {
        "background" => vec![(
            Property::BackgroundColor,
            Value::Color(value::color(&mut c)?),
        )],
        "font" => font_shorthand(&mut c)?,
        "margin" => sides(&mut c, Property::Margin, margin_value)?,
        "padding" => sides(&mut c, Property::Padding, padding_value)?,
        "border-width" => sides(&mut c, Property::BorderWidth, border_width)?,
        "border-color" => sides(&mut c, Property::BorderColor, |c| {
            Ok(Value::Color(value::color(c)?))
        })?,
        "border-style" => sides(&mut c, Property::BorderStyle, border_style)?,
        "border-radius" => corners(&mut c)?,
        "border" => border_shorthand(&mut c, &Side::ALL)?,
        "transition" => transition_shorthand(&mut c)?,
        _ => {
            if let Some(side) = Side::ALL
                .into_iter()
                .find(|s| name == format!("border-{}", s.name()))
            {
                border_shorthand(&mut c, &[side])?
            } else {
                let property = longhand(name).ok_or_else(|| unknown(name))?;
                vec![(property, longhand_value(property, &mut c)?)]
            }
        }
    };
    if !c.at_end() {
        return Err(format!("`{name}` has more than its value"));
    }
    Ok(out)
}

/// The properties `name` sets: itself, or a shorthand's.
fn properties_of(name: &str) -> Option<Vec<Property>> {
    let per_side = |f: fn(Side) -> Property| Some(Side::ALL.into_iter().map(f).collect());
    match name {
        "background" => Some(vec![Property::BackgroundColor]),
        "font" => Some(vec![
            Property::FontFamily,
            Property::FontSize,
            Property::FontWeight,
            Property::LineHeight,
        ]),
        "margin" => per_side(Property::Margin),
        "padding" => per_side(Property::Padding),
        "border-width" => per_side(Property::BorderWidth),
        "border-color" => per_side(Property::BorderColor),
        "border-style" => per_side(Property::BorderStyle),
        "transition" => Some(vec![
            Property::TransitionProperty,
            Property::TransitionDuration,
            Property::TransitionTimingFunction,
            Property::TransitionDelay,
        ]),
        "border-radius" => Some(
            Corner::ALL
                .into_iter()
                .map(Property::BorderRadius)
                .collect(),
        ),
        "border" => Some(
            Side::ALL
                .into_iter()
                .flat_map(|s| {
                    [
                        Property::BorderWidth(s),
                        Property::BorderStyle(s),
                        Property::BorderColor(s),
                    ]
                })
                .collect(),
        ),
        _ => Side::ALL
            .into_iter()
            .find(|s| name == format!("border-{}", s.name()))
            .map(|s| {
                vec![
                    Property::BorderWidth(s),
                    Property::BorderStyle(s),
                    Property::BorderColor(s),
                ]
            })
            .or_else(|| longhand(name).map(|p| vec![p])),
    }
}

/// A longhand's value.
fn longhand_value(property: Property, c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    match property {
        Property::Color | Property::BackgroundColor | Property::BorderColor(_) => {
            Ok(Value::Color(value::color(c)?))
        }
        Property::FontFamily => font_family(c),
        Property::FontSize => font_size(c),
        Property::FontWeight => font_weight(c),
        Property::LineHeight => line_height(c),
        Property::TextAlign => text_align(c),
        Property::Margin(_) => margin_value(c),
        Property::Padding(_) => padding_value(c),
        Property::BorderWidth(_) => border_width(c),
        Property::BorderStyle(_) => border_style(c),
        Property::BorderRadius(_) => non_negative(c, true),
        Property::Width | Property::Height => size_value(c, "auto", Value::Auto),
        Property::MinWidth | Property::MinHeight => {
            size_value(c, "auto", Value::Length(Length::default()))
        }
        Property::MaxWidth | Property::MaxHeight => size_value(c, "none", Value::None),
        Property::Opacity => opacity(c),
        Property::BoxShadow => shadow(c, true),
        Property::TextShadow => shadow(c, false),
        Property::Cursor => cursor(c),
        Property::TransitionProperty => transition_property(c),
        Property::TransitionDuration => list(c, |c| time(c, false)).map(Value::Times),
        Property::TransitionTimingFunction => list(c, timing).map(Value::Timings),
        Property::TransitionDelay => list(c, |c| time(c, true)).map(Value::Times),
    }
}

/// Whether the next token is the keyword `word`; taken if it is.
fn keyword(c: &mut Cursor<'_>, word: &str) -> bool {
    if matches!(c.peek(), Some(Token::Ident(w)) if w.eq_ignore_ascii_case(word)) {
        c.advance();
        true
    } else {
        false
    }
}

/// A length or percentage that may not be negative.
fn non_negative(c: &mut Cursor<'_>, percent: bool) -> Result<Value, ValueError> {
    let l = value::length(c, percent)?;
    if l.is_negative() {
        Err("a negative length where none may be".to_string())
    } else {
        Ok(Value::Length(l))
    }
}

fn margin_value(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    if keyword(c, "auto") {
        return Err("`auto` margins are not laid out: give a length".to_string());
    }
    Ok(Value::Length(value::length(c, true)?))
}

fn padding_value(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    non_negative(c, true)
}

fn border_width(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    for (word, px) in [("thin", 1.0), ("medium", 3.0), ("thick", 5.0)] {
        if keyword(c, word) {
            return Ok(Value::Length(Length::px(px)));
        }
    }
    non_negative(c, false)
}

fn border_style(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    match c.advance() {
        Some(Token::Ident(w)) => match w.to_ascii_lowercase().as_str() {
            "none" | "hidden" => Ok(Value::BorderStyle(BorderStyle::None)),
            "solid" | "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => {
                Ok(Value::BorderStyle(BorderStyle::Solid))
            }
            _ => Err(format!("`{w}` is not a border style")),
        },
        other => Err(format!("{other:?} where a border style was wanted")),
    }
}

fn size_value(c: &mut Cursor<'_>, word: &str, then: Value) -> Result<Value, ValueError> {
    if keyword(c, word) {
        return Ok(then);
    }
    non_negative(c, true)
}

fn opacity(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    let v = match c.peek() {
        Some(Token::Percentage(p)) => {
            let p = *p;
            c.advance();
            #[allow(clippy::cast_possible_truncation, reason = "a fraction")]
            let p = p as f32;
            p / 100.0
        }
        _ => value::number(c)?,
    };
    Ok(Value::Number(v.clamp(0.0, 1.0)))
}

fn text_align(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    match c.advance() {
        Some(Token::Ident(w)) => match w.to_ascii_lowercase().as_str() {
            "left" | "start" => Ok(Value::TextAlign(TextAlign::Left)),
            "right" | "end" => Ok(Value::TextAlign(TextAlign::Right)),
            "center" => Ok(Value::TextAlign(TextAlign::Center)),
            "justify" => Err("`justify` is not drawn: text is set ragged".to_string()),
            _ => Err(format!("`{w}` is not an alignment")),
        },
        other => Err(format!("{other:?} where an alignment was wanted")),
    }
}

fn cursor(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    let shape = match c.advance() {
        Some(Token::Ident(w)) => match w.to_ascii_lowercase().as_str() {
            "auto" | "default" => PointerCursor::Default,
            "pointer" => PointerCursor::Pointer,
            "text" => PointerCursor::Text,
            "crosshair" => PointerCursor::Crosshair,
            "move" | "grab" | "grabbing" => PointerCursor::Move,
            "ns-resize" | "row-resize" | "n-resize" | "s-resize" => PointerCursor::ResizeNS,
            "ew-resize" | "col-resize" | "e-resize" | "w-resize" => PointerCursor::ResizeEW,
            "nesw-resize" | "ne-resize" | "sw-resize" => PointerCursor::ResizeNESW,
            "nwse-resize" | "nw-resize" | "se-resize" => PointerCursor::ResizeNWSE,
            "not-allowed" | "no-drop" => PointerCursor::NotAllowed,
            "wait" | "progress" => PointerCursor::Wait,
            _ => return Err(format!("`{w}` is not a pointer shape this draws")),
        },
        other => return Err(format!("{other:?} where a pointer shape was wanted")),
    };
    Ok(Value::Cursor(shape))
}

/// One to four values, as CSS gives them to the four sides: all; top and
/// bottom, then the sides; top, the sides, bottom; each in turn.
fn sides(
    c: &mut Cursor<'_>,
    property: fn(Side) -> Property,
    each: fn(&mut Cursor<'_>) -> Result<Value, ValueError>,
) -> Result<Vec<(Property, Value)>, ValueError> {
    let mut values = Vec::new();
    while !c.at_end() && values.len() < 4 {
        values.push(each(c)?);
    }
    let pick = |i: usize| values.get(i).cloned();
    let (top, right, bottom, left) = match values.len() {
        1 => (pick(0), pick(0), pick(0), pick(0)),
        2 => (pick(0), pick(1), pick(0), pick(1)),
        3 => (pick(0), pick(1), pick(2), pick(1)),
        4 => (pick(0), pick(1), pick(2), pick(3)),
        _ => return Err("one to four values were wanted".to_string()),
    };
    Ok([
        (Side::Top, top),
        (Side::Right, right),
        (Side::Bottom, bottom),
        (Side::Left, left),
    ]
    .into_iter()
    .filter_map(|(s, v)| v.map(|v| (property(s), v)))
    .collect())
}

/// `border-radius`'s one to four radii, from the top left clockwise. The
/// elliptical form (`/`) is not read.
fn corners(c: &mut Cursor<'_>) -> Result<Vec<(Property, Value)>, ValueError> {
    let mut values = Vec::new();
    while !c.at_end() && values.len() < 4 {
        if matches!(c.peek(), Some(Token::Delim('/'))) {
            return Err("an elliptical corner (`/`) is not drawn".to_string());
        }
        values.push(non_negative(c, true)?);
    }
    let pick = |i: usize| values.get(i).cloned();
    let (tl, tr, br, bl) = match values.len() {
        1 => (pick(0), pick(0), pick(0), pick(0)),
        2 => (pick(0), pick(1), pick(0), pick(1)),
        3 => (pick(0), pick(1), pick(2), pick(1)),
        4 => (pick(0), pick(1), pick(2), pick(3)),
        _ => return Err("one to four radii were wanted".to_string()),
    };
    Ok([
        (Corner::TopLeft, tl),
        (Corner::TopRight, tr),
        (Corner::BottomRight, br),
        (Corner::BottomLeft, bl),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| (Property::BorderRadius(k), v)))
    .collect())
}

/// `border` or `border-<side>`: a width, a style and a colour, in any order,
/// each optional; what is left out is its initial value -- no border, the
/// text's colour, `medium`.
fn border_shorthand(c: &mut Cursor<'_>, to: &[Side]) -> Result<Vec<(Property, Value)>, ValueError> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    while !c.at_end() {
        let mut tried = *c;
        if width.is_none()
            && let Ok(w) = border_width(&mut tried)
        {
            width = Some(w);
            *c = tried;
            continue;
        }
        let mut tried = *c;
        if style.is_none()
            && let Ok(s) = border_style(&mut tried)
        {
            style = Some(s);
            *c = tried;
            continue;
        }
        let mut tried = *c;
        if color.is_none()
            && let Ok(col) = value::color(&mut tried)
        {
            color = Some(Value::Color(col));
            *c = tried;
            continue;
        }
        return Err(format!(
            "{:?} is not a border's width, style or colour",
            c.peek()
        ));
    }
    let width = width.unwrap_or(Value::Length(Length::px(3.0)));
    let style = style.unwrap_or(Value::BorderStyle(BorderStyle::None));
    let color = color.unwrap_or(Value::Color(ColorValue::CurrentColor));
    Ok(to
        .iter()
        .flat_map(|&s| {
            [
                (Property::BorderWidth(s), width.clone()),
                (Property::BorderStyle(s), style.clone()),
                (Property::BorderColor(s), color.clone()),
            ]
        })
        .collect())
}

fn font_family(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    let mut families = Vec::new();
    loop {
        let family = match c.advance() {
            Some(Token::Str(name)) => named_family(name),
            Some(Token::Ident(first)) => {
                // Unquoted names of several words: each a word, spaces between.
                let mut name = first.clone();
                while let Some(Token::Ident(more)) = c.peek() {
                    c.advance();
                    name.push(' ');
                    name.push_str(more);
                }
                generic_family(&name).unwrap_or_else(|| named_family(&name))
            }
            other => return Err(format!("{other:?} where a font family was wanted")),
        };
        families.push(family);
        match c.peek() {
            Some(Token::Comma) => {
                c.advance();
            }
            _ => return Ok(Value::FontFamily(families)),
        }
    }
}

fn generic_family(name: &str) -> Option<Family> {
    match name.to_ascii_lowercase().as_str() {
        "system-ui" | "ui-sans-serif" | "sans-serif" | "serif" | "ui-serif" | "cursive"
        | "fantasy" | "ui-rounded" => Some(Family::Ui),
        "monospace" | "ui-monospace" => Some(Family::Mono),
        _ => None,
    }
}

fn named_family(name: &str) -> Family {
    Family::Named(name.trim().to_string())
}

/// `font-size`: a length, a percentage of the parent's, or a keyword: the
/// absolute ones as a share of the base size (`medium`), and `larger` and
/// `smaller` relative to the parent's.
fn font_size(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    if let Some(Token::Ident(w)) = c.peek() {
        let rem = match w.to_ascii_lowercase().as_str() {
            "xx-small" => Some(0.6),
            "x-small" => Some(0.75),
            "small" => Some(8.0 / 9.0),
            "medium" => Some(1.0),
            "large" => Some(1.2),
            "x-large" => Some(1.5),
            "xx-large" => Some(2.0),
            "xxx-large" => Some(3.0),
            _ => None,
        };
        let em = match w.to_ascii_lowercase().as_str() {
            "larger" => Some(1.2),
            "smaller" => Some(1.0 / 1.2),
            _ => None,
        };
        if let Some(rem) = rem {
            c.advance();
            return Ok(Value::Length(Length {
                rem,
                ..Length::default()
            }));
        }
        if let Some(em) = em {
            c.advance();
            return Ok(Value::Length(Length {
                em,
                ..Length::default()
            }));
        }
    }
    // A percentage of the parent's size is a share of an em, for the font
    // size is the one property an em is the parent's for.
    let l = value::length(c, true)?;
    if l.is_negative() {
        return Err("a negative font size".to_string());
    }
    Ok(Value::Length(Length {
        em: l.em + l.percent / 100.0,
        percent: 0.0,
        ..l
    }))
}

/// `font-weight`: 1 to 1000, `normal`, `bold`, `bolder`, `lighter`.
fn font_weight(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    match c.peek() {
        Some(Token::Ident(w)) => {
            let w = w.to_ascii_lowercase();
            c.advance();
            match w.as_str() {
                "normal" => Ok(Value::FontWeight(400)),
                "bold" => Ok(Value::FontWeight(700)),
                "bolder" => Ok(Value::Bolder),
                "lighter" => Ok(Value::Lighter),
                _ => Err(format!("`{w}` is not a weight")),
            }
        }
        _ => {
            let n = value::number(c)?;
            if (1.0..=1000.0).contains(&n) {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked to be 1 to 1000"
                )]
                let n = n.round() as u16;
                Ok(Value::FontWeight(n))
            } else {
                Err("a weight is from 1 to 1000".to_string())
            }
        }
    }
}

/// `line-height`: `normal`, a multiple of the font size, or a length.
fn line_height(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    if keyword(c, "normal") {
        return Ok(Value::LineHeight(LineHeight::Normal));
    }
    if let Some(Token::Number(_)) = c.peek() {
        let n = value::number(c)?;
        return if n >= 0.0 {
            Ok(Value::LineHeight(LineHeight::Multiple(n)))
        } else {
            Err("a negative line height".to_string())
        };
    }
    let l = value::length(c, true)?;
    if l.is_negative() {
        return Err("a negative line height".to_string());
    }
    // A percentage is of the font size: a multiple.
    if l.percent != 0.0
        && l == (Length {
            percent: l.percent,
            ..Length::default()
        })
    {
        return Ok(Value::LineHeight(LineHeight::Multiple(l.percent / 100.0)));
    }
    Ok(Value::LineHeight(LineHeight::Length(Length {
        em: l.em + l.percent / 100.0,
        percent: 0.0,
        ..l
    })))
}

/// `box-shadow` or `text-shadow`: `none`, or one shadow -- two to four
/// lengths (two or three for text) and a colour, in either order. A list
/// of shadows draws its first; `inset` is not drawn.
fn shadow(c: &mut Cursor<'_>, boxed: bool) -> Result<Value, ValueError> {
    if keyword(c, "none") {
        return Ok(Value::None);
    }
    let mut lengths = Vec::new();
    let mut color = None;
    while !c.at_end() {
        if matches!(c.peek(), Some(Token::Comma)) {
            return Err("only one shadow is drawn".to_string());
        }
        if keyword(c, "inset") {
            return Err("an inset shadow is not drawn".to_string());
        }
        let mut tried = *c;
        if let Ok(l) = value::length(&mut tried, false) {
            lengths.push(l);
            *c = tried;
            continue;
        }
        if color.is_none() {
            color = Some(value::color(c)?);
            continue;
        }
        return Err(format!("{:?} in a shadow", c.peek()));
    }
    let most = if boxed { 4 } else { 3 };
    if lengths.len() < 2 || lengths.len() > most {
        return Err(format!("a shadow takes two to {most} lengths"));
    }
    let blur = lengths.get(2).copied().unwrap_or_default();
    if blur.is_negative() {
        return Err("a negative blur".to_string());
    }
    Ok(Value::Shadow(ShadowValue {
        x: lengths.first().copied().unwrap_or_default(),
        y: lengths.get(1).copied().unwrap_or_default(),
        blur,
        spread: lengths.get(3).copied().unwrap_or_default(),
        color: color.unwrap_or(ColorValue::CurrentColor),
    }))
}

/// `font`: `[weight] size[/line-height] family`, the weight optional. The
/// style (`italic`) and the variant are not drawn.
fn font_shorthand(c: &mut Cursor<'_>) -> Result<Vec<(Property, Value)>, ValueError> {
    let mut weight = Value::FontWeight(400);
    let mut line = Value::LineHeight(LineHeight::Normal);
    loop {
        match c.peek() {
            Some(Token::Ident(w))
                if w.eq_ignore_ascii_case("italic") || w.eq_ignore_ascii_case("oblique") =>
            {
                return Err("italic text is not drawn yet".to_string());
            }
            Some(Token::Ident(w))
                if ["normal", "bold", "bolder", "lighter"]
                    .contains(&w.to_ascii_lowercase().as_str()) =>
            {
                weight = font_weight(c)?;
            }
            Some(Token::Number(_)) => weight = font_weight(c)?,
            _ => break,
        }
    }
    let size = font_size(c)?;
    if matches!(c.peek(), Some(Token::Delim('/'))) {
        c.advance();
        line = line_height(c)?;
    }
    let family = font_family(c)?;
    Ok(vec![
        (Property::FontWeight, weight),
        (Property::FontSize, size),
        (Property::LineHeight, line),
        (Property::FontFamily, family),
    ])
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

/// Whether the next token is a comma; taken if it is.
fn comma(c: &mut Cursor<'_>) -> bool {
    if c.peek() == Some(&Token::Comma) {
        c.advance();
        true
    } else {
        false
    }
}

/// One or more of what `item` reads, separated by commas.
fn list<T>(
    c: &mut Cursor<'_>,
    mut item: impl FnMut(&mut Cursor<'_>) -> Result<T, ValueError>,
) -> Result<Vec<T>, ValueError> {
    let mut out = vec![item(c)?];
    while comma(c) {
        out.push(item(c)?);
    }
    Ok(out)
}

/// `transition-property`: `none`, or a list of `all` and properties.
fn transition_property(c: &mut Cursor<'_>) -> Result<Value, ValueError> {
    if keyword(c, "none") {
        return Ok(Value::Transitions(Vec::new()));
    }
    list(c, |c| match c.advance() {
        Some(Token::Ident(name)) => transition_target(name),
        other => Err(format!("{other:?} where a property was wanted")),
    })
    .map(Value::Transitions)
}

/// What one name in a `transition-property` list moves: `all`, a property
/// or a shorthand's, or -- a name this does not read -- nothing, kept so the
/// lists line up, as CSS keeps it.
fn transition_target(name: &str) -> Result<TransitionTarget, ValueError> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "all" => Ok(TransitionTarget::All),
        "none" | "inherit" | "initial" | "unset" | "default" => Err(format!(
            "`{lower}` names no property and cannot be one of a list"
        )),
        _ => Ok(properties_of(&lower).map_or(
            TransitionTarget::Unknown(lower),
            TransitionTarget::Properties,
        )),
    }
}

/// A time, in milliseconds: `200ms`, `0.2s`. A delay may be negative
/// (`negative`); a duration may not.
fn time(c: &mut Cursor<'_>, negative: bool) -> Result<f32, ValueError> {
    let ms = match c.advance() {
        Some(Token::Dimension(n, unit)) if unit.eq_ignore_ascii_case("ms") => value::narrow(*n),
        Some(Token::Dimension(n, unit)) if unit.eq_ignore_ascii_case("s") => {
            value::narrow(*n * 1000.0)
        }
        other => return Err(format!("{other:?} where a time (`ms`, `s`) was wanted")),
    };
    if !ms.is_finite() {
        return Err("a time too long to hold".to_string());
    }
    if ms < 0.0 && !negative {
        return Err("a negative duration".to_string());
    }
    Ok(ms)
}

/// Whether `word` is one of the timing functions' keywords.
fn is_timing_keyword(word: &str) -> bool {
    [
        "linear",
        "ease",
        "ease-in",
        "ease-out",
        "ease-in-out",
        "step-start",
        "step-end",
    ]
    .contains(&word.to_ascii_lowercase().as_str())
}

/// A timing function: a keyword, `cubic-bezier()` or `steps()`.
fn timing(c: &mut Cursor<'_>) -> Result<Timing, ValueError> {
    match c.advance() {
        Some(Token::Ident(word)) => match word.to_ascii_lowercase().as_str() {
            "linear" => Ok(Timing::Linear),
            "ease" => Ok(Timing::EASE),
            "ease-in" => Ok(Timing::EASE_IN),
            "ease-out" => Ok(Timing::EASE_OUT),
            "ease-in-out" => Ok(Timing::EASE_IN_OUT),
            "step-start" => Ok(Timing::Steps(1, StepPosition::JumpStart)),
            "step-end" => Ok(Timing::Steps(1, StepPosition::JumpEnd)),
            other => Err(format!("`{other}` is not a timing function")),
        },
        Some(Token::Function(f)) if f.eq_ignore_ascii_case("cubic-bezier") => {
            let args = c.take_block().ok_or("a `cubic-bezier(` is never closed")?;
            let mut a = Cursor::new(args);
            let numbers = list(&mut a, value::number)?;
            if !a.at_end() {
                return Err("`cubic-bezier()` has more than its four numbers".to_string());
            }
            let &[x1, y1, x2, y2] = numbers.as_slice() else {
                return Err("`cubic-bezier()` takes four numbers".to_string());
            };
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return Err("a `cubic-bezier()`'s x values are from 0 to 1".to_string());
            }
            if [y1, y2].iter().any(|y| !y.is_finite()) {
                return Err("a `cubic-bezier()` y that is not a number".to_string());
            }
            Ok(Timing::CubicBezier(x1, y1, x2, y2))
        }
        Some(Token::Function(f)) if f.eq_ignore_ascii_case("steps") => {
            let args = c.take_block().ok_or("a `steps(` is never closed")?;
            let mut a = Cursor::new(args);
            let n = value::number(&mut a)?;
            let position = if comma(&mut a) {
                match a.advance() {
                    Some(Token::Ident(w)) => match w.to_ascii_lowercase().as_str() {
                        "jump-start" | "start" => StepPosition::JumpStart,
                        "jump-end" | "end" => StepPosition::JumpEnd,
                        "jump-none" => StepPosition::JumpNone,
                        "jump-both" => StepPosition::JumpBoth,
                        other => return Err(format!("`{other}` is not where a step jumps")),
                    },
                    other => return Err(format!("{other:?} where a step's position was wanted")),
                }
            } else {
                StepPosition::JumpEnd
            };
            if !a.at_end() {
                return Err("`steps()` has more than its count and position".to_string());
            }
            let least = if position == StepPosition::JumpNone {
                2.0
            } else {
                1.0
            };
            if !n.is_finite() || n < least || n > f32::from(u16::MAX) || n.fract() > 0.0 {
                return Err(format!(
                    "`steps()` takes a whole number of steps from {least} to {}",
                    u16::MAX
                ));
            }
            // Whole and in u16's range: checked just above.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(Timing::Steps(n as u16, position))
        }
        other => Err(format!("{other:?} where a timing function was wanted")),
    }
}

/// `transition`: each transition's property, duration, timing function and
/// delay, in any order -- the first time its duration, the second its delay
/// -- read into the four lists, each a transition long. What one leaves out
/// is its initial value: `all`, `0s`, the desktop's curve, `0s`.
fn transition_shorthand(c: &mut Cursor<'_>) -> Result<Vec<(Property, Value)>, ValueError> {
    let mut properties = Vec::new();
    let mut durations = Vec::new();
    let mut timings = Vec::new();
    let mut delays = Vec::new();
    let mut none = false;
    loop {
        let (mut property, mut duration, mut ease, mut delay) = (None, None, None, None);
        loop {
            match c.peek() {
                None | Some(Token::Comma) => break,
                Some(Token::Dimension(..)) => {
                    if duration.is_none() {
                        duration = Some(time(c, false)?);
                    } else if delay.is_none() {
                        delay = Some(time(c, true)?);
                    } else {
                        return Err("a transition has more than two times".to_string());
                    }
                }
                Some(Token::Function(_)) if ease.is_none() => ease = Some(timing(c)?),
                Some(Token::Ident(w)) if ease.is_none() && is_timing_keyword(w) => {
                    ease = Some(timing(c)?);
                }
                Some(Token::Ident(w)) if property.is_none() => {
                    let w = w.clone();
                    c.advance();
                    property = Some(if w.eq_ignore_ascii_case("none") {
                        none = true;
                        None
                    } else {
                        Some(transition_target(&w)?)
                    });
                }
                Some(other) => return Err(format!("{other:?} in a transition")),
            }
        }
        if property.is_none() && duration.is_none() && ease.is_none() && delay.is_none() {
            return Err("a transition with nothing in it".to_string());
        }
        match property {
            None => properties.push(TransitionTarget::All),
            Some(Some(target)) => properties.push(target),
            // `none`: nothing moves.
            Some(None) => {}
        }
        durations.push(duration.unwrap_or(0.0));
        timings.push(ease.unwrap_or(Timing::Desktop));
        delays.push(delay.unwrap_or(0.0));
        if !comma(c) {
            break;
        }
    }
    // `none` moves nothing, and so is a list of its own.
    if none && durations.len() > 1 {
        return Err("`none` in a list of transitions".to_string());
    }
    Ok(vec![
        (Property::TransitionProperty, Value::Transitions(properties)),
        (Property::TransitionDuration, Value::Times(durations)),
        (Property::TransitionTimingFunction, Value::Timings(timings)),
        (Property::TransitionDelay, Value::Times(delays)),
    ])
}

#[cfg(test)]
#[path = "decl_tests.rs"]
mod tests;
