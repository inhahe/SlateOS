//! SVG (Subset) renderer for icon rendering and simple vector graphics.
//!
//! Supports enough of the SVG specification for typical application icons and
//! simple illustrations. Does NOT implement the full SVG 2.0 spec.
//!
//! # Supported subset
//!
//! - Basic shapes: rect, circle, ellipse, line, polyline, polygon
//! - Path element with full command set (M, L, H, V, C, S, Q, T, A, Z)
//! - Styling: fill, fill-rule, stroke, stroke-width, stroke-linecap,
//!   stroke-linejoin, stroke-miterlimit, opacity, display, transforms -- as
//!   presentation attributes or in a `style` attribute, which wins; a
//!   `<style>` sheet's rules are not applied
//! - Definitions (`defs`, `symbol`, gradients, clip paths, masks, `style`) are
//!   not drawn; nothing that refers to them (`use`, `url(#...)`) is supported
//! - Container elements: svg (with viewBox), g (with inheritance)
//! - Color parsing: hex, named colors, rgb(), rgba(), none, transparent, currentColor
//!
//! # Rasterizing
//!
//! A shape's fill, and separately its stroke, is one coverage pass: every edge
//! of every subpath takes part in one winding count, and each pixel is blended
//! once with the share of it the shape covers. A stroke is the union of a quad
//! per segment, a join per corner and a cap per open end, wound alike and
//! filled nonzero. Curves are cut finely enough for the size they are drawn
//! at, and a stroke's width scales with the drawing, as a length in user space
//! does.

// Geometry functions inherently need many coordinate parameters.
#![allow(clippy::too_many_arguments)]

use crate::color::Color;

use core::f32::consts::PI;

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during SVG parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgError {
    /// Invalid or malformed XML structure.
    MalformedXml(String),
    /// Invalid path data in a `d` attribute.
    InvalidPathData(String),
    /// Invalid color value.
    InvalidColor(String),
    /// Invalid transform string.
    InvalidTransform(String),
    /// Missing required attribute.
    MissingAttribute(String),
    /// Unsupported SVG feature encountered.
    Unsupported(String),
}

impl core::fmt::Display for SvgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedXml(msg) => write!(f, "malformed XML: {msg}"),
            Self::InvalidPathData(msg) => write!(f, "invalid path data: {msg}"),
            Self::InvalidColor(msg) => write!(f, "invalid color: {msg}"),
            Self::InvalidTransform(msg) => write!(f, "invalid transform: {msg}"),
            Self::MissingAttribute(msg) => write!(f, "missing attribute: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

// ─── Color Parsing ───────────────────────────────────────────────────────────

/// A parsed SVG paint value (fill or stroke).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SvgPaint {
    /// A solid color.
    Color(Color),
    /// No paint ("none").
    #[default]
    None,
    /// Inherit from parent context's foreground color ("currentColor").
    CurrentColor,
}

/// Parse an SVG color/paint string.
pub fn parse_color(s: &str) -> Result<SvgPaint, SvgError> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Ok(SvgPaint::None);
    }
    if s.eq_ignore_ascii_case("transparent") {
        return Ok(SvgPaint::Color(Color::TRANSPARENT));
    }
    if s.eq_ignore_ascii_case("currentColor") || s.eq_ignore_ascii_case("currentcolor") {
        return Ok(SvgPaint::CurrentColor);
    }

    // Named colors
    if let Some(c) = named_color(s) {
        return Ok(SvgPaint::Color(c));
    }

    // Hex colors
    if let Some(rest) = s.strip_prefix('#') {
        return parse_hex_color(rest).map(SvgPaint::Color);
    }

    // rgb()/rgba()
    if let Some(inner) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        return parse_rgba_func(inner).map(SvgPaint::Color);
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        return parse_rgb_func(inner).map(SvgPaint::Color);
    }

    Err(SvgError::InvalidColor(format!("unrecognized color: {s}")))
}

fn parse_hex_color(hex: &str) -> Result<Color, SvgError> {
    // Matching the bytes rather than testing `hex.len()` and then indexing:
    // the length check and the reads become one expression, so the six-digit
    // arm cannot be edited into reading a seventh byte.
    match *hex.as_bytes() {
        // #rgb is shorthand for #rrggbb: each digit is doubled, so #f0a is
        // #ff00aa.
        [r, g, b] => {
            let r = u8_from_hex_char(r)?;
            let g = u8_from_hex_char(g)?;
            let b = u8_from_hex_char(b)?;
            Ok(Color::rgb(r | (r << 4), g | (g << 4), b | (b << 4)))
        }
        [rh, rl, gh, gl, bh, bl] => Ok(Color::rgb(
            u8_from_hex_pair(rh, rl)?,
            u8_from_hex_pair(gh, gl)?,
            u8_from_hex_pair(bh, bl)?,
        )),
        [rh, rl, gh, gl, bh, bl, ah, al] => Ok(Color::rgba(
            u8_from_hex_pair(rh, rl)?,
            u8_from_hex_pair(gh, gl)?,
            u8_from_hex_pair(bh, bl)?,
            u8_from_hex_pair(ah, al)?,
        )),
        _ => Err(SvgError::InvalidColor(format!("bad hex length: #{hex}"))),
    }
}

fn u8_from_hex_char(c: u8) -> Result<u8, SvgError> {
    // `to_digit` rather than three ranged subtractions: it is the same
    // conversion without the arithmetic, and — being one call rather than a
    // match arm plus a subtraction that must agree with it — it cannot be made
    // to disagree with its own range check.
    if let Some(digit) = char::from(c).to_digit(16) {
        if let Ok(v) = u8::try_from(digit) {
            return Ok(v);
        }
    }
    // `c` is a *byte* of the colour string, and the bytes that reach here are
    // by definition the non-hex ones — including the continuation bytes of a
    // multi-byte character. `char::from(c)` would reinterpret such a byte as
    // Latin-1 and name a character the author never wrote (the first byte of
    // "ÿ" would be reported as "Ã"), so report the byte value for anything
    // outside printable ASCII.
    Err(SvgError::InvalidColor(if c.is_ascii_graphic() {
        format!("bad hex char: {}", char::from(c))
    } else {
        format!("bad hex byte: {c:#04x}")
    }))
}

fn u8_from_hex_pair(hi: u8, lo: u8) -> Result<u8, SvgError> {
    Ok(u8_from_hex_char(hi)? << 4 | u8_from_hex_char(lo)?)
}

fn parse_rgb_func(inner: &str) -> Result<Color, SvgError> {
    let parts: Vec<&str> = inner.split(',').collect();
    // The `else` arm carries the same condition the old `parts.len() != 3`
    // stated, but the three bindings come from the pattern that proved it
    // rather than from three separate indexes below.
    let [r, g, b] = parts.as_slice() else {
        return Err(SvgError::InvalidColor(format!(
            "rgb() expects 3 values: {inner}"
        )));
    };
    Ok(Color::rgb(
        parse_u8_component(r)?,
        parse_u8_component(g)?,
        parse_u8_component(b)?,
    ))
}

fn parse_rgba_func(inner: &str) -> Result<Color, SvgError> {
    let parts: Vec<&str> = inner.split(',').collect();
    let [r_str, g_str, b_str, a_str] = parts.as_slice() else {
        return Err(SvgError::InvalidColor(format!(
            "rgba() expects 4 values: {inner}"
        )));
    };
    let r = parse_u8_component(r_str)?;
    let g = parse_u8_component(g_str)?;
    let b = parse_u8_component(b_str)?;
    let a_str = a_str.trim();
    // Alpha can be 0.0-1.0 or 0-255
    let a = if a_str.contains('.') {
        let f: f32 = a_str
            .parse()
            .map_err(|_| SvgError::InvalidColor(format!("bad alpha: {a_str}")))?;
        (f.clamp(0.0, 1.0) * 255.0) as u8
    } else {
        parse_u8_component(a_str)?
    };
    Ok(Color::rgba(r, g, b, a))
}

fn parse_u8_component(s: &str) -> Result<u8, SvgError> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let f: f32 = pct
            .trim()
            .parse()
            .map_err(|_| SvgError::InvalidColor(format!("bad percentage: {s}")))?;
        Ok((f.clamp(0.0, 100.0) * 2.55) as u8)
    } else {
        let v: u32 = s
            .parse()
            .map_err(|_| SvgError::InvalidColor(format!("bad component: {s}")))?;
        Ok(v.min(255) as u8)
    }
}

fn named_color(name: &str) -> Option<Color> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "black" => Some(Color::rgb(0, 0, 0)),
        "white" => Some(Color::rgb(255, 255, 255)),
        "red" => Some(Color::rgb(255, 0, 0)),
        "green" => Some(Color::rgb(0, 128, 0)),
        "blue" => Some(Color::rgb(0, 0, 255)),
        "yellow" => Some(Color::rgb(255, 255, 0)),
        "cyan" | "aqua" => Some(Color::rgb(0, 255, 255)),
        "magenta" | "fuchsia" => Some(Color::rgb(255, 0, 255)),
        "orange" => Some(Color::rgb(255, 165, 0)),
        "purple" => Some(Color::rgb(128, 0, 128)),
        "gray" | "grey" => Some(Color::rgb(128, 128, 128)),
        "silver" => Some(Color::rgb(192, 192, 192)),
        "maroon" => Some(Color::rgb(128, 0, 0)),
        "olive" => Some(Color::rgb(128, 128, 0)),
        "teal" => Some(Color::rgb(0, 128, 128)),
        "navy" => Some(Color::rgb(0, 0, 128)),
        "lime" => Some(Color::rgb(0, 255, 0)),
        "pink" => Some(Color::rgb(255, 192, 203)),
        "brown" => Some(Color::rgb(165, 42, 42)),
        "coral" => Some(Color::rgb(255, 127, 80)),
        "gold" => Some(Color::rgb(255, 215, 0)),
        "indigo" => Some(Color::rgb(75, 0, 130)),
        "ivory" => Some(Color::rgb(255, 255, 240)),
        "khaki" => Some(Color::rgb(240, 230, 140)),
        "lavender" => Some(Color::rgb(230, 230, 250)),
        "salmon" => Some(Color::rgb(250, 128, 114)),
        "tan" => Some(Color::rgb(210, 180, 140)),
        "violet" => Some(Color::rgb(238, 130, 238)),
        "wheat" => Some(Color::rgb(245, 222, 179)),
        _ => None,
    }
}

// ─── Transform ───────────────────────────────────────────────────────────────

/// 2D affine transform stored as a 3x2 matrix (row-major):
/// ```text
/// | a  b  tx |
/// | c  d  ty |
/// | 0  0   1 |
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// How much this transform scales a length, as one number: the square
    /// root of how much it scales an area.
    ///
    /// Exact for a uniform scale and for a rotation. For a scale that differs
    /// by axis it is the geometric mean of the two -- the one number a length
    /// with no direction, such as a stroke's width, can honestly be given.
    #[must_use]
    pub fn length_scale(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }

    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translate(tx: f32, ty: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx,
            ty,
        }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn rotate(angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn matrix(a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> Self {
        Self { a, b, c, d, tx, ty }
    }

    /// Multiply self * other (apply other first, then self).
    pub fn then(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            tx: self.a * other.tx + self.b * other.ty + self.tx,
            ty: self.c * other.tx + self.d * other.ty + self.ty,
        }
    }

    /// Apply this transform to a point.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.b * y + self.tx,
            self.c * x + self.d * y + self.ty,
        )
    }
}

/// Parse an SVG transform attribute string.
/// Supports: translate, rotate, scale, matrix, skewX, skewY.
pub fn parse_transform(s: &str) -> Result<Transform, SvgError> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Transform::IDENTITY);
    }

    let mut result = Transform::IDENTITY;
    let mut remaining = s;

    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        // Split at the parentheses rather than searching for both from the
        // start and slicing between the two offsets. The old form called
        // `find(')')` on the *whole* remainder, so an input whose ')' came
        // before its '(' — `)x(1,2)` — produced a start index past its end and
        // panicked inside the slice. Splitting makes the ordering structural:
        // the closing paren is looked for only in what follows the opening one.
        let (head, after_open) = remaining
            .split_once('(')
            .ok_or_else(|| SvgError::InvalidTransform(format!("expected '(' in: {remaining}")))?;
        let func_name = head.trim();
        let (args_str, after_close) = after_open
            .split_once(')')
            .ok_or_else(|| SvgError::InvalidTransform(format!("expected ')' in: {remaining}")))?;
        remaining = after_close;

        // Skip optional comma/whitespace separators between transforms
        remaining = remaining.trim_start_matches(|c: char| c == ',' || c.is_whitespace());

        let args = parse_transform_args(args_str)?;

        let t = match func_name {
            "translate" => {
                let tx = args.first().copied().unwrap_or(0.0);
                let ty = args.get(1).copied().unwrap_or(0.0);
                Transform::translate(tx, ty)
            }
            "scale" => {
                let sx = args.first().copied().unwrap_or(1.0);
                let sy = args.get(1).copied().unwrap_or(sx);
                Transform::scale(sx, sy)
            }
            "rotate" => {
                let angle = args.first().copied().unwrap_or(0.0) * PI / 180.0;
                // `rotate(angle, cx, cy)` turns about a point; `rotate(angle)`
                // turns about the origin. Reading the centre out of the slice
                // pattern ties the "are there three?" test to the two values
                // that test is there to justify.
                if let [_, cx, cy, ..] = *args.as_slice() {
                    Transform::translate(cx, cy)
                        .then(Transform::rotate(angle))
                        .then(Transform::translate(-cx, -cy))
                } else {
                    Transform::rotate(angle)
                }
            }
            "matrix" => {
                let [a, b, c, d, tx, ty, ..] = *args.as_slice() else {
                    return Err(SvgError::InvalidTransform(
                        "matrix() requires 6 values".into(),
                    ));
                };
                Transform::matrix(a, b, c, d, tx, ty)
            }
            "skewX" => {
                let angle = args.first().copied().unwrap_or(0.0) * PI / 180.0;
                Transform {
                    a: 1.0,
                    b: angle.tan(),
                    c: 0.0,
                    d: 1.0,
                    tx: 0.0,
                    ty: 0.0,
                }
            }
            "skewY" => {
                let angle = args.first().copied().unwrap_or(0.0) * PI / 180.0;
                Transform {
                    a: 1.0,
                    b: 0.0,
                    c: angle.tan(),
                    d: 1.0,
                    tx: 0.0,
                    ty: 0.0,
                }
            }
            _ => {
                return Err(SvgError::InvalidTransform(format!(
                    "unknown transform: {func_name}"
                )));
            }
        };

        result = result.then(t);
    }

    Ok(result)
}

fn parse_transform_args(s: &str) -> Result<Vec<f32>, SvgError> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            seg.trim()
                .parse::<f32>()
                .map_err(|_| SvgError::InvalidTransform(format!("bad number: {seg}")))
        })
        .collect()
}

// ─── Path Data ───────────────────────────────────────────────────────────────

/// A single command in a parsed SVG path, with absolute coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo {
        x: f32,
        y: f32,
    },
    LineTo {
        x: f32,
        y: f32,
    },
    HorizontalLineTo {
        x: f32,
    },
    VerticalLineTo {
        y: f32,
    },
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x: f32,
        y: f32,
    },
    SmoothCubic {
        x2: f32,
        y2: f32,
        x: f32,
        y: f32,
    },
    QuadraticBezier {
        x1: f32,
        y1: f32,
        x: f32,
        y: f32,
    },
    SmoothQuadratic {
        x: f32,
        y: f32,
    },
    Arc {
        rx: f32,
        ry: f32,
        x_rotation: f32,
        large_arc: bool,
        sweep: bool,
        x: f32,
        y: f32,
    },
    Close,
}

/// A cursor over the tokens of a path `d` attribute.
///
/// The parser used to carry a bare `let mut i = 0` beside the token slice and
/// move it by hand: `i += 1` appeared twenty-four times, and every read was a
/// `tokens[i]` that was in bounds only because a `while i < tokens.len()`
/// several lines above said so — a proof living in a different statement from
/// the code it justifies, which a later edit is free to invalidate silently.
///
/// That is the same shape as the wire-decode cursor in `guiremote`, and it has
/// the same fix: one type that owns the position, hands out only
/// bounds-checked reads, and is the only thing that may advance it.
struct PathTokens<'a> {
    tokens: &'a [String],
    at: usize,
}

impl<'a> PathTokens<'a> {
    fn new(tokens: &'a [String]) -> Self {
        Self { tokens, at: 0 }
    }

    /// The token under the cursor, without consuming it.
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.at).map(String::as_str)
    }

    /// The token under the cursor, consuming it.
    fn next_token(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.bump();
        Some(token)
    }

    /// Move past the token that was just read.
    ///
    /// `saturating_add` is a formality — the cursor is only ever compared
    /// against a `Vec` length, which cannot reach `usize::MAX` — but it means
    /// this is not the place an overflow could occur.
    fn bump(&mut self) {
        self.at = self.at.saturating_add(1);
    }

    /// Is the next token a number?
    ///
    /// This is what continues a command's implicit-repetition loop: `L 1 2 3 4`
    /// is two linetos, and `M 1 2 3 4` is a moveto followed by one.
    fn at_number(&self) -> bool {
        self.peek().is_some_and(is_number_token)
    }

    /// Consume exactly `N` numbers.
    ///
    /// Returning `[f32; N]` rather than `Vec<f32>` is the point. The helper
    /// this replaces returned a `Vec`, so every caller had to take it back
    /// apart by index — `(vals[0], vals[1], vals[2], vals[3])` appeared eight
    /// times and the six-element form six more — and nothing but a runtime
    /// bound tied the count the caller asked for to the count it then read.
    /// With an array the two cannot disagree: the length is in the type, and
    /// destructuring it is exhaustive.
    ///
    /// A failure leaves the cursor wherever it stopped, which is harmless
    /// because the error aborts the whole parse.
    fn take_numbers<const N: usize>(&mut self, what: &str) -> Result<[f32; N], SvgError> {
        let mut out = [0.0f32; N];
        for (found, slot) in out.iter_mut().enumerate() {
            let Some(token) = self.peek().filter(|t| is_number_token(t)) else {
                return Err(SvgError::InvalidPathData(format!(
                    "{what}: expected {N} numbers, found {found}"
                )));
            };
            *slot = parse_path_number(token)?;
            self.bump();
        }
        Ok(out)
    }
}

/// Resolve a coordinate pair against the current point.
///
/// Relative commands (the lowercase letters) treat their arguments as offsets
/// from where the pen is; absolute ones replace it. Spelling that out once
/// removes six copies of the same six-line `if is_relative` block, one of
/// which had to repeat `cursor_x + vals[n]` three times over.
fn resolve(is_relative: bool, from_x: f32, from_y: f32, x: f32, y: f32) -> (f32, f32) {
    if is_relative {
        (from_x + x, from_y + y)
    } else {
        (x, y)
    }
}

/// The single letter of a one-character token, if that is what it is.
///
/// The old form tested `token.len() == 1 && token.as_bytes()[0].is_ascii_alphabetic()`
/// and then indexed `token.as_bytes()[0]` a second time to read the byte back
/// out. A slice pattern states the length check and the read as one thing, so
/// they cannot drift apart.
fn single_ascii_alphabetic(token: &str) -> Option<u8> {
    match token.as_bytes() {
        [only] if only.is_ascii_alphabetic() => Some(*only),
        _ => None,
    }
}

/// Parse an SVG path `d` attribute into a list of absolute `PathCommand`s.
///
/// # Errors
///
/// Returns [`SvgError::InvalidPathData`] if the data contains an unknown
/// command letter, a token that is neither a command nor a number, or a
/// command with fewer arguments than it requires.
pub fn parse_path_data(d: &str) -> Result<Vec<PathCommand>, SvgError> {
    let mut commands = Vec::new();
    let mut cursor_x: f32 = 0.0;
    let mut cursor_y: f32 = 0.0;
    let mut start_x: f32 = 0.0;
    let mut start_y: f32 = 0.0;

    let tokens = tokenize_path(d);
    let mut t = PathTokens::new(&tokens);

    while let Some(token) = t.next_token() {
        let Some(cmd_char) = single_ascii_alphabetic(token) else {
            // Bare numbers where a command letter was expected: an implicit
            // lineto. Note these are taken as absolute regardless of the
            // preceding command's case, which is what this parser has always
            // done; the case that reaches here is numbers at the very start of
            // the data or straight after a `Z`, neither of which the grammar
            // actually allows, so there is no "preceding command" to inherit
            // relativity from.
            if !is_number_token(token) {
                return Err(SvgError::InvalidPathData(format!(
                    "unexpected token: {token}"
                )));
            }
            let x = parse_path_number(token)?;
            let [y] = t.take_numbers("implicit lineto")?;
            commands.push(PathCommand::LineTo { x, y });
            cursor_x = x;
            cursor_y = y;
            continue;
        };

        let is_relative = cmd_char.is_ascii_lowercase();

        match cmd_char.to_ascii_uppercase() {
            b'M' => {
                // The first pair is the moveto; any that follow it are implicit
                // linetos (SVG 1.1 section 8.3.2).
                let mut first = true;
                while t.at_number() {
                    let [x_raw, y_raw] = t.take_numbers("M")?;
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, x_raw, y_raw);
                    if first {
                        commands.push(PathCommand::MoveTo { x, y });
                        start_x = x;
                        start_y = y;
                        first = false;
                    } else {
                        commands.push(PathCommand::LineTo { x, y });
                    }
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'L' => {
                while t.at_number() {
                    let [x_raw, y_raw] = t.take_numbers("L")?;
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, x_raw, y_raw);
                    commands.push(PathCommand::LineTo { x, y });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'H' => {
                while t.at_number() {
                    let [x_raw] = t.take_numbers("H")?;
                    let x = if is_relative { cursor_x + x_raw } else { x_raw };
                    commands.push(PathCommand::HorizontalLineTo { x });
                    cursor_x = x;
                }
            }
            b'V' => {
                while t.at_number() {
                    let [y_raw] = t.take_numbers("V")?;
                    let y = if is_relative { cursor_y + y_raw } else { y_raw };
                    commands.push(PathCommand::VerticalLineTo { y });
                    cursor_y = y;
                }
            }
            b'C' => {
                while t.at_number() {
                    let [x1r, y1r, x2r, y2r, xr, yr] = t.take_numbers("C")?;
                    let (x1, y1) = resolve(is_relative, cursor_x, cursor_y, x1r, y1r);
                    let (x2, y2) = resolve(is_relative, cursor_x, cursor_y, x2r, y2r);
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, xr, yr);
                    commands.push(PathCommand::CubicBezier {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'S' => {
                while t.at_number() {
                    let [x2r, y2r, xr, yr] = t.take_numbers("S")?;
                    let (x2, y2) = resolve(is_relative, cursor_x, cursor_y, x2r, y2r);
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, xr, yr);
                    commands.push(PathCommand::SmoothCubic { x2, y2, x, y });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'Q' => {
                while t.at_number() {
                    let [x1r, y1r, xr, yr] = t.take_numbers("Q")?;
                    let (x1, y1) = resolve(is_relative, cursor_x, cursor_y, x1r, y1r);
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, xr, yr);
                    commands.push(PathCommand::QuadraticBezier { x1, y1, x, y });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'T' => {
                while t.at_number() {
                    let [xr, yr] = t.take_numbers("T")?;
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, xr, yr);
                    commands.push(PathCommand::SmoothQuadratic { x, y });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'A' => {
                while t.at_number() {
                    let [rx, ry, x_rotation, large_arc, sweep, xr, yr] = t.take_numbers("A")?;
                    let (x, y) = resolve(is_relative, cursor_x, cursor_y, xr, yr);
                    commands.push(PathCommand::Arc {
                        // A negative radius is out of range rather than a parse
                        // error: SVG 1.1 section 8.3.8 says to take its absolute
                        // value and carry on, not to reject the path.
                        rx: rx.abs(),
                        ry: ry.abs(),
                        x_rotation,
                        large_arc: large_arc != 0.0,
                        sweep: sweep != 0.0,
                        x,
                        y,
                    });
                    cursor_x = x;
                    cursor_y = y;
                }
            }
            b'Z' => {
                commands.push(PathCommand::Close);
                cursor_x = start_x;
                cursor_y = start_y;
            }
            _ => {
                return Err(SvgError::InvalidPathData(format!(
                    "unknown command: {}",
                    char::from(cmd_char)
                )));
            }
        }
    }

    Ok(commands)
}

/// Tokenize path data into commands and numbers.
/// Handles negative numbers adjacent to commands (e.g., "M10-5" -> "M", "10", "-5").
fn tokenize_path(d: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    // A byte iterator rather than a hand-moved index. The loop only ever went
    // forward one byte at a time and never looked back, so the index bought
    // nothing at all — it just added an `i += 1` to every one of eight arms
    // (an omission in any of which is an infinite loop) and a `bytes[i]` that
    // the iterator makes unnecessary.
    for c in d.bytes() {
        match c {
            // A command letter always starts a new token.
            b'M' | b'm' | b'L' | b'l' | b'H' | b'h' | b'V' | b'v' | b'C' | b'c' | b'S' | b's'
            | b'Q' | b'q' | b'T' | b't' | b'A' | b'a' | b'Z' | b'z' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                tokens.push(String::from(char::from(c)));
            }
            // Separators.
            b',' | b' ' | b'\t' | b'\n' | b'\r' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
            }
            // A minus doubles as a separator: `10-5` is two numbers.
            b'-' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                current.push('-');
            }
            // So does a second dot: `.5.5` is two numbers.
            b'.' => {
                if current.contains('.') {
                    tokens.push(core::mem::take(&mut current));
                }
                current.push('.');
            }
            // Digits, and `e`/`E` for scientific notation.
            b'0'..=b'9' | b'e' | b'E' => current.push(char::from(c)),
            b'+' => {
                // A plus is part of an exponent (`1e+3`); anywhere else it
                // introduces a new number.
                if current.ends_with('e') || current.ends_with('E') {
                    current.push('+');
                } else if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
            }
            // Anything else: skip it, but end whatever was in progress.
            _ => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Could this token begin a number? (Whether it *is* one is
/// [`parse_path_number`]'s question.)
fn is_number_token(s: &str) -> bool {
    // A slice pattern rather than an emptiness check followed by `[0]`: the
    // guard and the read are one expression, so there is no way to reach the
    // read without the guard.
    matches!(s.as_bytes(), [first, ..]
        if *first == b'-' || *first == b'+' || *first == b'.' || first.is_ascii_digit())
}

fn parse_path_number(s: &str) -> Result<f32, SvgError> {
    s.parse::<f32>()
        .map_err(|_| SvgError::InvalidPathData(format!("bad number: {s}")))
}

// ─── SVG Node Tree ───────────────────────────────────────────────────────────

/// How the inside of a filled shape is decided where its outline crosses
/// itself or one subpath lies inside another (`fill-rule`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// Inside where the outline winds around the point at all: a subpath
    /// inside another, drawn the same way round, fills rather than cuts.
    #[default]
    NonZero,
    /// Inside where the outline crosses an odd number of times on the way
    /// out: every nested subpath alternates between filled and hole.
    EvenOdd,
}

/// The shape of a stroke's open ends (`stroke-linecap`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Cut square at the end point.
    #[default]
    Butt,
    /// A half disc past the end point.
    Round,
    /// Carried on past the end point by half the stroke's width.
    Square,
}

/// The shape of a stroke where two of its segments meet (`stroke-linejoin`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// The outer edges carried on to meet in a point, unless that point is
    /// further out than `stroke-miterlimit` allows; then a bevel.
    #[default]
    Miter,
    /// A disc around the corner.
    Round,
    /// The outer corners joined straight across.
    Bevel,
}

/// Style properties for an SVG node.
///
/// `None` in a field is "not said here", which inherits the parent's value.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgStyle {
    pub fill: Option<SvgPaint>,
    pub fill_rule: Option<FillRule>,
    pub stroke: Option<SvgPaint>,
    pub stroke_width: Option<f32>,
    pub stroke_linecap: Option<LineCap>,
    pub stroke_linejoin: Option<LineJoin>,
    /// At least 1, as SVG requires; a smaller value is not said.
    pub stroke_miterlimit: Option<f32>,
    pub opacity: f32,
    pub fill_opacity: f32,
    pub stroke_opacity: f32,
}

impl Default for SvgStyle {
    fn default() -> Self {
        Self {
            fill: None,
            fill_rule: None,
            stroke: None,
            stroke_width: None,
            stroke_linecap: None,
            stroke_linejoin: None,
            stroke_miterlimit: None,
            opacity: 1.0,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
        }
    }
}

/// A node in the SVG document tree.
#[derive(Clone, Debug)]
pub enum SvgNode {
    Svg {
        width: Option<f32>,
        height: Option<f32>,
        view_box: Option<(f32, f32, f32, f32)>,
        children: Vec<SvgNode>,
    },
    Group {
        transform: Transform,
        style: SvgStyle,
        children: Vec<SvgNode>,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rx: f32,
        ry: f32,
        transform: Transform,
        style: SvgStyle,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        transform: Transform,
        style: SvgStyle,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        transform: Transform,
        style: SvgStyle,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        transform: Transform,
        style: SvgStyle,
    },
    Polyline {
        points: Vec<(f32, f32)>,
        transform: Transform,
        style: SvgStyle,
    },
    Polygon {
        points: Vec<(f32, f32)>,
        transform: Transform,
        style: SvgStyle,
    },
    Path {
        commands: Vec<PathCommand>,
        transform: Transform,
        style: SvgStyle,
    },
}

// ─── SVG Document ────────────────────────────────────────────────────────────

/// A parsed SVG document ready for rendering.
#[derive(Clone, Debug)]
pub struct SvgDocument {
    pub root: SvgNode,
}

impl SvgDocument {
    /// Parse an SVG string into a document tree.
    pub fn parse(svg_data: &str) -> Result<Self, SvgError> {
        let elements = parse_xml(svg_data)?;
        // `first` rather than an `is_empty` check followed by `[0]`: the check
        // and the read are one expression, so they cannot drift apart.
        let first = elements
            .first()
            .ok_or_else(|| SvgError::MalformedXml("empty document".into()))?;
        let root = build_node(first)?;
        Ok(Self { root })
    }

    /// Get the viewBox (min_x, min_y, width, height).
    /// Returns (0, 0, width, height) if no explicit viewBox is set.
    pub fn viewbox(&self) -> (f32, f32, f32, f32) {
        if let SvgNode::Svg {
            view_box,
            width,
            height,
            ..
        } = &self.root
        {
            if let Some(vb) = view_box {
                return *vb;
            }
            return (0.0, 0.0, width.unwrap_or(300.0), height.unwrap_or(150.0));
        }
        (0.0, 0.0, 300.0, 150.0)
    }

    /// Render the SVG to a pixel buffer at the given dimensions: 4 bytes per
    /// pixel, `[r, g, b, a]`, straight alpha, row by row.
    /// Uses 4x supersampling for anti-aliased edges.
    pub fn render(&self, width: u32, height: u32) -> Vec<u8> {
        let mut renderer = SvgRenderer::new(width, height);
        let (vb_x, vb_y, vb_w, vb_h) = self.viewbox();
        let scale_x = width as f32 / vb_w;
        let scale_y = height as f32 / vb_h;
        let base_transform =
            Transform::scale(scale_x, scale_y).then(Transform::translate(-vb_x, -vb_y));
        renderer.render_node(&self.root, base_transform, &ResolvedStyle::default());
        renderer.buffer
    }
}

// ─── Resolved (Inherited) Style ──────────────────────────────────────────────

/// Style with inheritance resolved — used during rendering traversal.
#[derive(Clone, Debug)]
struct ResolvedStyle {
    fill: SvgPaint,
    fill_rule: FillRule,
    stroke: SvgPaint,
    /// In user space: `paint` scales it to the drawing.
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f32,
    opacity: f32,
    fill_opacity: f32,
    stroke_opacity: f32,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        // SVG's initial values.
        Self {
            fill: SvgPaint::Color(Color::BLACK),
            fill_rule: FillRule::NonZero,
            stroke: SvgPaint::None,
            stroke_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            opacity: 1.0,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
        }
    }
}

impl ResolvedStyle {
    fn with_overrides(&self, style: &SvgStyle) -> Self {
        Self {
            fill: style.fill.unwrap_or(self.fill),
            fill_rule: style.fill_rule.unwrap_or(self.fill_rule),
            stroke: style.stroke.unwrap_or(self.stroke),
            stroke_width: style.stroke_width.unwrap_or(self.stroke_width),
            line_cap: style.stroke_linecap.unwrap_or(self.line_cap),
            line_join: style.stroke_linejoin.unwrap_or(self.line_join),
            miter_limit: style.stroke_miterlimit.unwrap_or(self.miter_limit),
            opacity: self.opacity * style.opacity,
            fill_opacity: style.fill_opacity,
            stroke_opacity: style.stroke_opacity,
        }
    }

    fn effective_fill_color(&self) -> Option<Color> {
        match self.fill {
            SvgPaint::Color(c) => {
                let alpha = (c.a as f32 * self.opacity * self.fill_opacity) as u8;
                Some(Color::rgba(c.r, c.g, c.b, alpha))
            }
            SvgPaint::CurrentColor => {
                // Fallback to black for currentColor
                let alpha = (255.0 * self.opacity * self.fill_opacity) as u8;
                Some(Color::rgba(0, 0, 0, alpha))
            }
            SvgPaint::None => None,
        }
    }

    fn effective_stroke_color(&self) -> Option<Color> {
        match self.stroke {
            SvgPaint::Color(c) => {
                let alpha = (c.a as f32 * self.opacity * self.stroke_opacity) as u8;
                Some(Color::rgba(c.r, c.g, c.b, alpha))
            }
            SvgPaint::CurrentColor => {
                let alpha = (255.0 * self.opacity * self.stroke_opacity) as u8;
                Some(Color::rgba(0, 0, 0, alpha))
            }
            SvgPaint::None => None,
        }
    }
}

// ─── XML Parser (minimal, SVG-only) ─────────────────────────────────────────

/// A minimal XML element for SVG parsing.
#[derive(Clone, Debug)]
struct XmlElement {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlElement>,
}

impl XmlElement {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn attr_f32(&self, name: &str) -> Option<f32> {
        self.attr(name).and_then(|s| s.parse::<f32>().ok())
    }
}

/// A cursor over the bytes of an XML document.
///
/// The five functions below used to take `(bytes: &[u8], pos: &mut usize)` —
/// a cursor split into two loose parameters that nothing kept together. Every
/// read was `bytes[*pos]` guarded by a `*pos < bytes.len()` written out again
/// at each site (thirty-odd times), and every step was a `*pos += 1` that eight
/// separate loops each had to remember. Neither the bound nor the advance was
/// anywhere the compiler could enforce them.
///
/// Making the pair a type puts both in one place: `peek` returns `None` at the
/// end rather than needing a length test beside it, and `bump`/`advance` are
/// the only things that move, so they are the only things that can move past
/// the end — which they cannot, being clamped.
struct XmlCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> XmlCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// The byte under the cursor, or `None` at the end of the document.
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// The byte `n` positions further on, for the two-byte lookaheads (`</`,
    /// `<!`, `<?`) that decide what kind of thing is starting.
    fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.at.saturating_add(n)).copied()
    }

    /// The unread remainder.
    fn rest(&self) -> &'a [u8] {
        // `get` rather than a slice: the invariant says `at <= len`, and an
        // empty tail is the honest answer if it ever is not.
        self.bytes.get(self.at..).unwrap_or(&[])
    }

    /// Does the document continue with exactly these bytes?
    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.rest().starts_with(prefix)
    }

    fn at_end(&self) -> bool {
        self.peek().is_none()
    }

    fn bump(&mut self) {
        self.advance(1);
    }

    /// Move on `n` bytes, stopping at the end of the document.
    ///
    /// Clamping is what makes every `peek` above safe without a length test of
    /// its own: the cursor cannot be put past the end in the first place.
    fn advance(&mut self, n: usize) {
        self.at = self.at.saturating_add(n).min(self.bytes.len());
    }

    /// Abandon the rest of the document (an unterminated comment swallows it).
    fn finish(&mut self) {
        self.at = self.bytes.len();
    }

    /// Consume the byte under the cursor if it is `want`, reporting whether it
    /// was. Callers that merely tolerate its absence ignore the answer.
    fn eat(&mut self, want: u8) -> bool {
        let found = self.peek() == Some(want);
        if found {
            self.bump();
        }
        found
    }

    fn skip_while(&mut self, keep: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&keep) {
            self.bump();
        }
    }

    fn skip_whitespace(&mut self) {
        self.skip_while(|b| b.is_ascii_whitespace());
    }

    /// Consume bytes while `keep` holds and return them as text.
    ///
    /// Lossy, because an SVG file is not required to be valid UTF-8 and a tag
    /// or attribute name that is not is still better named by its replacement
    /// characters than by rejecting the whole document.
    fn take_while(&mut self, keep: impl Fn(u8) -> bool) -> String {
        let start = self.at;
        self.skip_while(keep);
        String::from_utf8_lossy(self.bytes.get(start..self.at).unwrap_or(&[])).into_owned()
    }
}

/// Parse minimal SVG-subset XML.
fn parse_xml(input: &str) -> Result<Vec<XmlElement>, SvgError> {
    let mut c = XmlCursor::new(input.as_bytes());

    // Skip the BOM, XML declaration, DOCTYPE and any comments before the root.
    skip_prolog(&mut c);

    let mut elements = Vec::new();
    loop {
        c.skip_whitespace();
        let Some(byte) = c.peek() else { break };
        if byte != b'<' {
            // Text content: this parser has no use for text nodes.
            c.bump();
            continue;
        }
        match c.peek_at(1) {
            Some(b'/') => break, // a closing tag — the caller's business
            Some(b'!' | b'?') => skip_special(&mut c),
            _ => elements.push(parse_element(&mut c)?),
        }
    }

    Ok(elements)
}

fn skip_prolog(c: &mut XmlCursor) {
    loop {
        c.skip_whitespace();
        let Some(byte) = c.peek() else { break };
        if byte != b'<' {
            c.bump();
        } else if matches!(c.peek_at(1), Some(b'?' | b'!')) {
            skip_special(c);
        } else {
            // The root element: the prolog is over.
            break;
        }
    }
}

/// Skip a `<!-- comment -->`, a `<?processing instruction?>` or a `<!DOCTYPE>`.
///
/// Always consumes at least one byte, which is what stops its callers' loops.
fn skip_special(c: &mut XmlCursor) {
    if c.starts_with(b"<!--") {
        c.advance(4);
        while !c.at_end() {
            if c.starts_with(b"-->") {
                c.advance(3);
                return;
            }
            c.bump();
        }
        // Unterminated: the remainder of the document is inside the comment.
        c.finish();
        return;
    }

    // A processing instruction or DOCTYPE, skipped to its matching '>'. The
    // depth count is for DOCTYPE's internal subset, which may contain '<'.
    c.bump();
    let mut depth = 1usize;
    while depth > 0 {
        let Some(byte) = c.peek() else { break };
        match byte {
            b'<' => depth = depth.saturating_add(1),
            b'>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        c.bump();
    }
}

fn parse_element(c: &mut XmlCursor) -> Result<XmlElement, SvgError> {
    if !c.eat(b'<') {
        return Err(SvgError::MalformedXml("expected '<'".into()));
    }

    let tag = c.take_while(|b| !b.is_ascii_whitespace() && b != b'>' && b != b'/');

    let mut attrs = Vec::new();
    loop {
        c.skip_whitespace();
        // End of document, or the '/>' or '>' that ends the open tag.
        if matches!(c.peek(), None | Some(b'/' | b'>')) {
            break;
        }
        let name =
            c.take_while(|b| b != b'=' && !b.is_ascii_whitespace() && b != b'>' && b != b'/');
        c.skip_whitespace();
        if c.eat(b'=') {
            c.skip_whitespace();
            let value = parse_attr_value(c)?;
            attrs.push((name, value));
        } else {
            // A valueless attribute — rare in SVG, but taken rather than
            // refused. Note this consumed the name, so the loop still advances.
            attrs.push((name, String::new()));
        }
    }

    let mut children = Vec::new();
    if c.eat(b'/') {
        // Self-closing, `<tag ... />`. If the document ends before the '>',
        // the element is still what it is; there is nothing to recover.
        c.eat(b'>');
    } else if c.eat(b'>') {
        loop {
            c.skip_whitespace();
            let Some(byte) = c.peek() else { break };
            if byte != b'<' {
                // Text content, which this parser has no use for.
                c.bump();
                continue;
            }
            match c.peek_at(1) {
                Some(b'/') => {
                    // Our closing tag. Skip its name and the '>' after it; the
                    // name is not checked against ours, which is why a
                    // mismatched document parses rather than erroring.
                    c.advance(2);
                    c.skip_while(|b| b != b'>');
                    c.eat(b'>');
                    break;
                }
                Some(b'!' | b'?') => skip_special(c),
                _ => children.push(parse_element(c)?),
            }
        }
    }

    Ok(XmlElement {
        tag,
        attrs,
        children,
    })
}

fn parse_attr_value(c: &mut XmlCursor) -> Result<String, SvgError> {
    let Some(quote) = c.peek() else {
        return Err(SvgError::MalformedXml("expected attribute value".into()));
    };

    if quote != b'"' && quote != b'\'' {
        // An unquoted value: not well-formed XML, but taken rather than refused.
        return Ok(c.take_while(|b| !b.is_ascii_whitespace() && b != b'>' && b != b'/'));
    }

    c.bump(); // the opening quote
    let value = c.take_while(|b| b != quote);
    c.eat(quote); // the closing quote, if the document has one
    Ok(value)
}

// ─── Node Builder ────────────────────────────────────────────────────────────

/// Elements whose contents are definitions for something else to use, or not
/// graphics at all: drawn directly, a `<symbol>`'s shapes or a `<defs>`' would
/// appear where nothing placed them, and a `<style>` sheet's text is not a
/// shape. Nothing here draws what refers to them, so they draw nothing.
const NOT_DRAWN: &[&str] = &[
    "defs",
    "symbol",
    "clipPath",
    "mask",
    "marker",
    "pattern",
    "linearGradient",
    "radialGradient",
    "filter",
    "style",
    "script",
    "metadata",
    "title",
    "desc",
];

/// An element that draws nothing.
fn nothing() -> SvgNode {
    SvgNode::Group {
        transform: Transform::IDENTITY,
        style: SvgStyle::default(),
        children: Vec::new(),
    }
}

fn build_node(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    // `display: none` takes the element and everything in it out of the
    // drawing, and a definition is not drawn where it stands.
    if NOT_DRAWN.contains(&elem.tag.as_str())
        || property(elem, "display").is_some_and(|value| value == "none")
    {
        return Ok(nothing());
    }
    match elem.tag.as_str() {
        "svg" => build_svg(elem),
        "g" => build_group(elem),
        "rect" => build_rect(elem),
        "circle" => build_circle(elem),
        "ellipse" => build_ellipse(elem),
        "line" => build_line(elem),
        "polyline" => build_polyline(elem),
        "polygon" => build_polygon(elem),
        "path" => build_path(elem),
        _ => {
            // Unknown elements treated as groups (e.g., <defs>, <title>)
            let children: Result<Vec<_>, _> = elem.children.iter().map(build_node).collect();
            Ok(SvgNode::Group {
                transform: Transform::IDENTITY,
                style: SvgStyle::default(),
                children: children?,
            })
        }
    }
}

fn build_svg(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    let width = elem.attr_f32("width");
    let height = elem.attr_f32("height");
    let view_box = elem.attr("viewBox").and_then(|s| parse_viewbox(s).ok());

    let children: Vec<SvgNode> = elem
        .children
        .iter()
        .map(build_node)
        .collect::<Result<_, _>>()?;
    // Presentation attributes on the root element -- `fill="none"
    // stroke="currentColor"` is how most icon sets are written -- are inherited
    // by everything in the document, as SVG says. They were ignored, so every
    // such icon drew its outlines as solid black shapes: the default fill,
    // unstroked. A group carrying them gives them the inheritance a `<g>`
    // already has, without a second kind of node to walk.
    let style = parse_style_attrs(elem)?;
    let children = if style == SvgStyle::default() {
        children
    } else {
        vec![SvgNode::Group {
            transform: Transform::IDENTITY,
            style,
            children,
        }]
    };
    Ok(SvgNode::Svg {
        width,
        height,
        view_box,
        children,
    })
}

fn build_group(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    let transform = elem
        .attr("transform")
        .map(parse_transform)
        .transpose()?
        .unwrap_or(Transform::IDENTITY);
    let style = parse_style_attrs(elem)?;
    let children: Result<Vec<_>, _> = elem.children.iter().map(build_node).collect();
    Ok(SvgNode::Group {
        transform,
        style,
        children: children?,
    })
}

fn build_rect(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    // One radius given is both, as SVG has it: `rx="2"` alone rounds the
    // corners, where reading the missing `ry` as 0 left them square.
    let (rx, ry) = match (elem.attr_f32("rx"), elem.attr_f32("ry")) {
        (Some(rx), Some(ry)) => (rx, ry),
        (Some(r), None) | (None, Some(r)) => (r, r),
        (None, None) => (0.0, 0.0),
    };
    Ok(SvgNode::Rect {
        x: elem.attr_f32("x").unwrap_or(0.0),
        y: elem.attr_f32("y").unwrap_or(0.0),
        width: elem.attr_f32("width").unwrap_or(0.0),
        height: elem.attr_f32("height").unwrap_or(0.0),
        rx,
        ry,
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_circle(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    Ok(SvgNode::Circle {
        cx: elem.attr_f32("cx").unwrap_or(0.0),
        cy: elem.attr_f32("cy").unwrap_or(0.0),
        r: elem.attr_f32("r").unwrap_or(0.0),
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_ellipse(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    Ok(SvgNode::Ellipse {
        cx: elem.attr_f32("cx").unwrap_or(0.0),
        cy: elem.attr_f32("cy").unwrap_or(0.0),
        rx: elem.attr_f32("rx").unwrap_or(0.0),
        ry: elem.attr_f32("ry").unwrap_or(0.0),
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_line(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    Ok(SvgNode::Line {
        x1: elem.attr_f32("x1").unwrap_or(0.0),
        y1: elem.attr_f32("y1").unwrap_or(0.0),
        x2: elem.attr_f32("x2").unwrap_or(0.0),
        y2: elem.attr_f32("y2").unwrap_or(0.0),
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_polyline(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    let points = elem
        .attr("points")
        .map(parse_points)
        .transpose()?
        .unwrap_or_default();
    Ok(SvgNode::Polyline {
        points,
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_polygon(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    let points = elem
        .attr("points")
        .map(parse_points)
        .transpose()?
        .unwrap_or_default();
    Ok(SvgNode::Polygon {
        points,
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

fn build_path(elem: &XmlElement) -> Result<SvgNode, SvgError> {
    let d = elem.attr("d").unwrap_or("");
    let commands = parse_path_data(d)?;
    Ok(SvgNode::Path {
        commands,
        transform: elem
            .attr("transform")
            .map(parse_transform)
            .transpose()?
            .unwrap_or(Transform::IDENTITY),
        style: parse_style_attrs(elem)?,
    })
}

/// The value of the style property `name` on `elem`: its declaration in the
/// element's `style` attribute, which CSS ranks above the presentation
/// attribute of the same name, or else that attribute.
///
/// Inkscape writes every property in `style`, and so does most of what is
/// drawn with it -- Breeze's icons among them -- so a renderer that read the
/// attributes alone drew those icons as solid black shapes.
fn property<'e>(elem: &'e XmlElement, name: &str) -> Option<&'e str> {
    elem.attr("style")
        .and_then(|style| declared(style, name))
        .or_else(|| elem.attr(name).map(str::trim))
}

/// The value the CSS declarations in `style` give `name` -- the last one, as a
/// later declaration overrides an earlier -- without its `!important`.
fn declared<'s>(style: &'s str, name: &str) -> Option<&'s str> {
    style
        .split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            (property.trim() == name).then(|| {
                let value = value.trim();
                value.strip_suffix("!important").unwrap_or(value).trim()
            })
        })
        .rfind(|value| !value.is_empty())
}

/// A length in user units: a number, in `px` or with no unit. Other units say
/// nothing this renderer can measure, and are not said.
fn length(value: &str) -> Option<f32> {
    let value = value.trim();
    value
        .strip_suffix("px")
        .unwrap_or(value)
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
}

/// A paint the element says, if it says one.
///
/// A presentation attribute this renderer cannot read is an error, as it has
/// always been. A declaration in `style` that it cannot read -- a gradient's
/// `url(#...)`, a colour function it does not know -- is ignored, as a browser
/// ignores it, and the property is inherited: one unreadable declaration among
/// the dozen Inkscape writes should not cost the whole drawing.
fn paint_property(elem: &XmlElement, name: &str) -> Result<Option<SvgPaint>, SvgError> {
    if let Some(value) = elem.attr("style").and_then(|style| declared(style, name)) {
        if let Ok(paint) = parse_color(value) {
            return Ok(Some(paint));
        }
    }
    elem.attr(name).map(parse_color).transpose()
}

fn parse_style_attrs(elem: &XmlElement) -> Result<SvgStyle, SvgError> {
    let fill = paint_property(elem, "fill")?;
    let stroke = paint_property(elem, "stroke")?;
    let stroke_width = property(elem, "stroke-width").and_then(length);
    let number = |name: &str| property(elem, name).and_then(|v| v.parse::<f32>().ok());
    let opacity = number("opacity").unwrap_or(1.0);
    let fill_opacity = number("fill-opacity").unwrap_or(1.0);
    let stroke_opacity = number("stroke-opacity").unwrap_or(1.0);
    // Keywords a renderer does not know are ignored, as browsers ignore them:
    // the property is then not said here and is inherited.
    let fill_rule = keyword(
        property(elem, "fill-rule"),
        &[
            ("nonzero", FillRule::NonZero),
            ("evenodd", FillRule::EvenOdd),
        ],
    );
    let stroke_linecap = keyword(
        property(elem, "stroke-linecap"),
        &[
            ("butt", LineCap::Butt),
            ("round", LineCap::Round),
            ("square", LineCap::Square),
        ],
    );
    let stroke_linejoin = keyword(
        property(elem, "stroke-linejoin"),
        &[
            ("miter", LineJoin::Miter),
            // SVG 2's clipped miter: the nearest this renderer draws.
            ("miter-clip", LineJoin::Miter),
            ("round", LineJoin::Round),
            ("bevel", LineJoin::Bevel),
        ],
    );
    let stroke_miterlimit =
        number("stroke-miterlimit").filter(|limit| limit.is_finite() && *limit >= 1.0);

    Ok(SvgStyle {
        fill,
        fill_rule,
        stroke,
        stroke_width,
        stroke_linecap,
        stroke_linejoin,
        stroke_miterlimit,
        opacity,
        fill_opacity,
        stroke_opacity,
    })
}

/// The value `table` gives the keyword `value` names, if it names one.
fn keyword<T: Copy>(value: Option<&str>, table: &[(&str, T)]) -> Option<T> {
    let value = value?.trim();
    table
        .iter()
        .find(|(name, _)| *name == value)
        .map(|&(_, meaning)| meaning)
}

fn parse_viewbox(s: &str) -> Result<(f32, f32, f32, f32), SvgError> {
    let parts: Vec<f32> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SvgError::MalformedXml(format!("bad viewBox: {s}")))?;
    // A slice pattern states the count once, where the values are bound. The
    // `len() != 4` form stated it twice — in the check and again in the four
    // indexes — and only the first of those was checked.
    let [min_x, min_y, width, height] = *parts.as_slice() else {
        return Err(SvgError::MalformedXml(format!(
            "viewBox needs 4 values: {s}"
        )));
    };
    Ok((min_x, min_y, width, height))
}

fn parse_points(s: &str) -> Result<Vec<(f32, f32)>, SvgError> {
    let numbers: Vec<f32> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| seg.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SvgError::InvalidPathData(format!("bad points: {s}")))?;
    if !numbers.len().is_multiple_of(2) {
        return Err(SvgError::InvalidPathData(
            "points needs even number of values".into(),
        ));
    }
    // `chunks_exact` rather than `chunks`: it yields only full pairs, so the
    // pattern below is the exhaustive case rather than a length assumption. The
    // `_` arm is unreachable for that reason, not merely unlikely.
    Ok(numbers
        .chunks_exact(2)
        .filter_map(|pair| match *pair {
            [x, y] => Some((x, y)),
            _ => None,
        })
        .collect())
}

// ─── Bézier Flattening ───────────────────────────────────────────────────────

/// Default flatness threshold in pixels.
const DEFAULT_FLATNESS: f32 = 0.25;

/// Flatten a cubic Bézier curve to line segments using adaptive subdivision.
fn flatten_cubic(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    flatness: f32,
    output: &mut Vec<(f32, f32)>,
) {
    flatten_cubic_recursive(
        x0,
        y0,
        x1,
        y1,
        x2,
        y2,
        x3,
        y3,
        flatness * flatness,
        MAX_SUBDIVISIONS,
        output,
    );
}

/// How many times a curve may be halved before we accept the chord as-is.
///
/// Each level doubles the segment count, so 17 is already 131072 segments —
/// far past anything a display can resolve. The cap exists to bound the stack
/// on degenerate curves (cusps, coincident control points) where the flatness
/// test can fail to converge, not to bound quality.
const MAX_SUBDIVISIONS: u32 = 17;

fn flatten_cubic_recursive(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    flatness_sq: f32,
    budget: u32,
    output: &mut Vec<(f32, f32)>,
) {
    // A budget spent downwards rather than a depth counted upwards: the
    // recursion cannot outlive its bound, because the bound *is* the value
    // being passed down. Counting up and comparing against a separate constant
    // put the limit and the counter in two places, either of which could be
    // changed without the other.
    let Some(remaining) = budget.checked_sub(1) else {
        output.push((x3, y3));
        return;
    };

    // Check flatness: distance of control points from the line (x0,y0)-(x3,y3)
    let dx = x3 - x0;
    let dy = y3 - y0;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-10 {
        output.push((x3, y3));
        return;
    }

    let d1 = ((x1 - x0) * dy - (y1 - y0) * dx).abs();
    let d2 = ((x2 - x0) * dy - (y2 - y0) * dx).abs();
    let dist_sq = (d1 + d2) * (d1 + d2) / len_sq;

    if dist_sq <= flatness_sq {
        output.push((x3, y3));
        return;
    }

    // Subdivide at t=0.5
    let mx01 = (x0 + x1) * 0.5;
    let my01 = (y0 + y1) * 0.5;
    let mx12 = (x1 + x2) * 0.5;
    let my12 = (y1 + y2) * 0.5;
    let mx23 = (x2 + x3) * 0.5;
    let my23 = (y2 + y3) * 0.5;
    let mx012 = (mx01 + mx12) * 0.5;
    let my012 = (my01 + my12) * 0.5;
    let mx123 = (mx12 + mx23) * 0.5;
    let my123 = (my12 + my23) * 0.5;
    let mx0123 = (mx012 + mx123) * 0.5;
    let my0123 = (my012 + my123) * 0.5;

    flatten_cubic_recursive(
        x0,
        y0,
        mx01,
        my01,
        mx012,
        my012,
        mx0123,
        my0123,
        flatness_sq,
        remaining,
        output,
    );
    flatten_cubic_recursive(
        mx0123,
        my0123,
        mx123,
        my123,
        mx23,
        my23,
        x3,
        y3,
        flatness_sq,
        remaining,
        output,
    );
}

/// Flatten a quadratic Bézier curve to line segments.
fn flatten_quadratic(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    flatness: f32,
    output: &mut Vec<(f32, f32)>,
) {
    // Convert to cubic: cubic control points are at 2/3 along tangent from endpoints
    let cx1 = x0 + (x1 - x0) * (2.0 / 3.0);
    let cy1 = y0 + (y1 - y0) * (2.0 / 3.0);
    let cx2 = x2 + (x1 - x2) * (2.0 / 3.0);
    let cy2 = y2 + (y1 - y2) * (2.0 / 3.0);
    flatten_cubic(x0, y0, cx1, cy1, cx2, cy2, x2, y2, flatness, output);
}

/// Approximate an elliptical arc with line segments.
fn flatten_arc(
    cursor_x: f32,
    cursor_y: f32,
    rx: f32,
    ry: f32,
    x_rotation: f32,
    large_arc: bool,
    sweep: bool,
    target_x: f32,
    target_y: f32,
    scale: f32,
    output: &mut Vec<(f32, f32)>,
) {
    // Implementation of the SVG arc endpoint-to-center parameterization
    // Reference: https://www.w3.org/TR/SVG/implnote.html#ArcImplementationNotes

    let mut rx = rx.abs();
    let mut ry = ry.abs();
    if rx < 1e-10 || ry < 1e-10 {
        output.push((target_x, target_y));
        return;
    }

    let phi = x_rotation * PI / 180.0;
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();

    // Step 1: compute (x1', y1')
    let dx2 = (cursor_x - target_x) / 2.0;
    let dy2 = (cursor_y - target_y) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    // Step 2: compute (cx', cy') — correct radii if needed
    let x1p_sq = x1p * x1p;
    let y1p_sq = y1p * y1p;
    let mut rx_sq = rx * rx;
    let mut ry_sq = ry * ry;

    let lambda = x1p_sq / rx_sq + y1p_sq / ry_sq;
    if lambda > 1.0 {
        let sqrt_lambda = lambda.sqrt();
        rx *= sqrt_lambda;
        ry *= sqrt_lambda;
        rx_sq = rx * rx;
        ry_sq = ry * ry;
    }

    let num = (rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq).max(0.0);
    let denom = rx_sq * y1p_sq + ry_sq * x1p_sq;
    let sq = if denom < 1e-10 {
        0.0
    } else {
        (num / denom).sqrt()
    };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = sign * sq * (rx * y1p / ry);
    let cyp = sign * sq * -(ry * x1p / rx);

    // Step 3: compute (cx, cy) from (cx', cy')
    // `midpoint` rather than `(a + b) / 2.0`: it cannot overflow to infinity on
    // the way to a result that is representable, which the sum can for endpoints
    // near `f32::MAX` — reachable here because the coordinates come from a file.
    let cx = cos_phi * cxp - sin_phi * cyp + f32::midpoint(cursor_x, target_x);
    let cy = sin_phi * cxp + cos_phi * cyp + f32::midpoint(cursor_y, target_y);

    // Step 4: compute theta1 and delta_theta
    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle_between(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );

    if !sweep && dtheta > 0.0 {
        dtheta -= 2.0 * PI;
    } else if sweep && dtheta < 0.0 {
        dtheta += 2.0 * PI;
    }

    // Approximate with line segments, as many as the arc needs at the size it
    // is drawn -- `scale` carries its radius to device pixels. It was a fixed
    // one per eighth of a turn, which a large icon showed as corners.
    let n_segs = curve_segments(rx.max(ry) * scale, dtheta);
    let step = dtheta / n_segs as f32;

    for i in 1..=n_segs {
        let theta = theta1 + step * i as f32;
        let cos_t = theta.cos();
        let sin_t = theta.sin();
        let px = cos_phi * rx * cos_t - sin_phi * ry * sin_t + cx;
        let py = sin_phi * rx * cos_t + cos_phi * ry * sin_t + cy;
        output.push((px, py));
    }
}

fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    if len < 1e-10 {
        return 0.0;
    }
    let cos_angle = (dot / len).clamp(-1.0, 1.0);
    let angle = cos_angle.acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

// ─── Geometry ────────────────────────────────────────────────────────────────

/// How far a flattened circle or arc may stray from the true curve, in device
/// pixels.
///
/// A tenth of a pixel is invisible at any size. Arcs used to be cut into a
/// fixed eight segments per turn and circles into thirty-two, whatever their
/// size, so a ring drawn a few hundred pixels across came out as a visible
/// polygon.
const CURVE_TOLERANCE_PX: f32 = 0.1;

/// The most segments one circle or arc is cut into, however large it is drawn.
const MAX_CURVE_SEGMENTS: u32 = 1024;

/// Points closer than this, in device pixels, are one point: a segment shorter
/// has no direction to stroke along.
const SAME_POINT_PX: f32 = 1e-4;

/// How many straight segments a curve of `radius` device pixels, turning
/// through `sweep` radians, needs to stay within [`CURVE_TOLERANCE_PX`].
fn curve_segments(radius: f32, sweep: f32) -> u32 {
    let radius = radius.abs();
    let sweep = sweep.abs();
    if !radius.is_finite() || !sweep.is_finite() {
        return MAX_CURVE_SEGMENTS;
    }
    if radius <= CURVE_TOLERANCE_PX {
        // Smaller than the tolerance: any polygon is within it. A quarter
        // turn a segment keeps the shape's extent.
        return ((sweep / (PI / 2.0)).ceil() as u32).clamp(1, 4);
    }
    // A chord spanning `step` radians strays `radius * (1 - cos(step / 2))`
    // from the arc it cuts off.
    let step = 2.0 * (1.0 - CURVE_TOLERANCE_PX / radius).acos();
    ((sweep / step).ceil() as u32).clamp(1, MAX_CURVE_SEGMENTS)
}

/// One run of a shape's outline in device pixels, and whether it closes on
/// itself.
///
/// The closing matters to the stroke and not to the fill: a fill treats every
/// run as closed, as SVG does, but a closed run is stroked with a join where it
/// meets itself and an open one with a cap at each end.
#[derive(Clone, Debug, Default, PartialEq)]
struct Subpath {
    points: Vec<(f32, f32)>,
    closed: bool,
}

/// Everything a stroke needs besides its outline, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StrokeGeometry {
    width: f32,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f32,
}

fn vec_sub(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

fn vec_add(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 + b.0, a.1 + b.1)
}

fn vec_scale(v: (f32, f32), k: f32) -> (f32, f32) {
    (v.0 * k, v.1 * k)
}

/// `v` scaled to length one, or `None` when it has no direction.
fn unit(v: (f32, f32)) -> Option<(f32, f32)> {
    let len = v.0.hypot(v.1);
    (len.is_finite() && len > SAME_POINT_PX).then(|| (v.0 / len, v.1 / len))
}

/// `d` turned a quarter turn: the side of a segment its stroke spreads to.
fn normal(d: (f32, f32)) -> (f32, f32) {
    (-d.1, d.0)
}

/// Twice the signed area of `poly`: its sign is the way it winds.
fn signed_area(poly: &[(f32, f32)]) -> f32 {
    let closing = match poly {
        [first, .., last] => Some((*last, *first)),
        _ => None,
    };
    poly.windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some((a, b)),
            _ => None,
        })
        .chain(closing)
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum()
}

/// A circle of `radius` around `centre`, as a polygon fine enough for its
/// size.
fn disk(centre: (f32, f32), radius: f32) -> Vec<(f32, f32)> {
    let n = curve_segments(radius, 2.0 * PI).max(8);
    (0..n)
        .map(|i| {
            let angle = 2.0 * PI * (i as f32 / n as f32);
            (
                centre.0 + radius * angle.cos(),
                centre.1 + radius * angle.sin(),
            )
        })
        .collect()
}

/// `points` without its non-finite points, and with each run of points too
/// close to tell apart kept once.
fn distinct_points(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for &p in points {
        if !p.0.is_finite() || !p.1.is_finite() {
            continue;
        }
        let repeat = out
            .last()
            .is_some_and(|&last| unit(vec_sub(p, last)).is_none());
        if !repeat {
            out.push(p);
        }
    }
    out
}

/// The polygon that fills the outer corner where a stroke turns at `at`,
/// coming in along `d_in` and going out along `d_out` (both unit length), or
/// `None` where it runs straight on.
fn join_polygon(
    at: (f32, f32),
    d_in: (f32, f32),
    d_out: (f32, f32),
    half: f32,
    join: LineJoin,
    miter_limit: f32,
) -> Option<Vec<(f32, f32)>> {
    let cross = d_in.0 * d_out.1 - d_in.1 * d_out.0;
    let dot = d_in.0 * d_out.0 + d_in.1 * d_out.1;
    if cross.abs() < 1e-6 && dot > 0.0 {
        return None;
    }
    if join == LineJoin::Round {
        return Some(disk(at, half));
    }
    // The corner opens on the side away from the turn: the two segments'
    // edges on that side leave a wedge the quads do not cover.
    let side = if cross > 0.0 { -half } else { half };
    let (n_in, n_out) = (normal(d_in), normal(d_out));
    let from = vec_add(at, vec_scale(n_in, side));
    let to = vec_add(at, vec_scale(n_out, side));
    if join == LineJoin::Miter {
        // The miter's length over the stroke's width is 1 / cos(turn / 2),
        // and |n_in + n_out| is 2 cos(turn / 2).
        let bisector = vec_add(n_in, n_out);
        let length = bisector.0.hypot(bisector.1);
        if length > 1e-6 {
            let ratio = 2.0 / length;
            if ratio <= miter_limit {
                let tip = vec_add(at, vec_scale(bisector, side * ratio / length));
                return Some(vec![at, from, tip, to]);
            }
        }
    }
    Some(vec![at, from, to])
}

/// The polygons whose union is `subpaths` stroked as `stroke` says: one quad
/// per segment, a join where segments meet, a cap at each open end.
///
/// Every polygon comes out wound the same way, so filling them together under
/// the nonzero rule fills their union and covers an overlap once. They used to
/// be drawn one quad at a time, each blended on its own: where two quads met
/// on a curve, each covered part of a pixel and the pixel never became solid,
/// so a circle's outline was drawn at half strength and a translucent stroke
/// darkened at every joint.
fn stroke_polygons(subpaths: &[Subpath], stroke: StrokeGeometry) -> Vec<Vec<(f32, f32)>> {
    let half = stroke.width / 2.0;
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    if half.is_nan() || half.is_infinite() || half <= 0.0 {
        return out;
    }
    for subpath in subpaths {
        let mut points = distinct_points(&subpath.points);
        if subpath.closed && points.len() > 1 {
            // A closed run that ends where it started has said the start twice.
            let back_home = match points.as_slice() {
                [first, .., last] => unit(vec_sub(*last, *first)).is_none(),
                _ => false,
            };
            if back_home {
                points.pop();
            }
        }
        match points.as_slice() {
            [] => continue,
            // A run with no length has no direction: SVG draws it as its caps
            // alone, a dot for a round cap and a square for a square one.
            [only] => {
                match stroke.cap {
                    LineCap::Butt => {}
                    LineCap::Round => out.push(disk(*only, half)),
                    LineCap::Square => out.push(vec![
                        (only.0 - half, only.1 - half),
                        (only.0 + half, only.1 - half),
                        (only.0 + half, only.1 + half),
                        (only.0 - half, only.1 + half),
                    ]),
                }
                continue;
            }
            _ => {}
        }
        // Each segment, as its two ends and its direction.
        let pairs: Vec<((f32, f32), (f32, f32))> = if subpath.closed {
            points
                .iter()
                .copied()
                .zip(points.iter().copied().cycle().skip(1))
                .collect()
        } else {
            points
                .windows(2)
                .filter_map(|w| match *w {
                    [a, b] => Some((a, b)),
                    _ => None,
                })
                .collect()
        };
        let directions: Vec<(f32, f32)> = pairs
            .iter()
            .map(|&(a, b)| unit(vec_sub(b, a)).unwrap_or((1.0, 0.0)))
            .collect();
        let last_segment = pairs.len().saturating_sub(1);
        for (index, (&(start, end), &direction)) in pairs.iter().zip(&directions).enumerate() {
            let (mut start, mut end) = (start, end);
            if !subpath.closed && stroke.cap == LineCap::Square {
                // A square cap is the segment carried on by half the width.
                if index == 0 {
                    start = vec_sub(start, vec_scale(direction, half));
                }
                if index == last_segment {
                    end = vec_add(end, vec_scale(direction, half));
                }
            }
            let offset = vec_scale(normal(direction), half);
            out.push(vec![
                vec_add(start, offset),
                vec_add(end, offset),
                vec_sub(end, offset),
                vec_sub(start, offset),
            ]);
        }
        // Joins: at every vertex of a closed run, at the inner ones of an open.
        let turns = directions.iter().zip(directions.iter().skip(1));
        let closing_turn = if subpath.closed {
            directions.last().copied().zip(directions.first().copied())
        } else {
            None
        };
        let vertices = pairs.iter().map(|&(_, end)| end);
        for (at, (d_in, d_out)) in vertices.zip(turns.map(|(a, b)| (*a, *b)).chain(closing_turn)) {
            if let Some(polygon) =
                join_polygon(at, d_in, d_out, half, stroke.join, stroke.miter_limit)
            {
                out.push(polygon);
            }
        }
        if !subpath.closed && stroke.cap == LineCap::Round {
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                out.push(disk(*first, half));
                out.push(disk(*last, half));
            }
        }
    }
    // One winding for all, so the nonzero rule sums overlaps instead of
    // letting two opposite windings cancel into a hole.
    for polygon in &mut out {
        if signed_area(polygon) > 0.0 {
            polygon.reverse();
        }
    }
    out
}

/// The outline of an axis-aligned rectangle in user space, rounded by `rx` by
/// `ry`, carried to device space by `transform`.
fn rect_subpath(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rx: f32,
    ry: f32,
    transform: Transform,
) -> Option<Subpath> {
    // SVG draws nothing for a rectangle with no width or no height.
    if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rx = rx.max(0.0).min(w / 2.0);
    let ry = ry.max(0.0).min(h / 2.0);
    let points = if rx <= 0.0 || ry <= 0.0 {
        vec![
            transform.apply(x, y),
            transform.apply(x + w, y),
            transform.apply(x + w, y + h),
            transform.apply(x, y + h),
        ]
    } else {
        let n = curve_segments(rx.max(ry) * transform.length_scale(), PI / 2.0);
        // The corners clockwise from the top right, each a quarter turn
        // starting where the last ended.
        let corners = [
            (x + w - rx, y + ry, -PI / 2.0),
            (x + w - rx, y + h - ry, 0.0),
            (x + rx, y + h - ry, PI / 2.0),
            (x + rx, y + ry, PI),
        ];
        let mut points = Vec::new();
        for (cx, cy, from) in corners {
            for i in 0..=n {
                let angle = from + (PI / 2.0) * (i as f32 / n as f32);
                points.push(transform.apply(cx + rx * angle.cos(), cy + ry * angle.sin()));
            }
        }
        points
    };
    Some(Subpath {
        points,
        closed: true,
    })
}

/// The outline of an ellipse in user space, carried to device space.
fn ellipse_subpath(cx: f32, cy: f32, rx: f32, ry: f32, transform: Transform) -> Option<Subpath> {
    // SVG draws nothing for an ellipse with a radius of zero.
    if rx.is_nan() || ry.is_nan() || rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let n = curve_segments(rx.max(ry) * transform.length_scale(), 2.0 * PI).max(8);
    let points = (0..n)
        .map(|i| {
            let angle = 2.0 * PI * (i as f32 / n as f32);
            transform.apply(cx + rx * angle.cos(), cy + ry * angle.sin())
        })
        .collect();
    Some(Subpath {
        points,
        closed: true,
    })
}

/// A run of user-space points carried to device space.
fn points_subpath(points: &[(f32, f32)], transform: Transform, closed: bool) -> Subpath {
    Subpath {
        points: points.iter().map(|&(x, y)| transform.apply(x, y)).collect(),
        closed,
    }
}

/// Convert path commands into the runs of points they draw, in device space.
///
/// A run begins at each moveto, and after each closepath when anything is
/// drawn before the next moveto -- it begins at the closed run's start, as SVG
/// has it. A moveto that nothing is drawn from is not a run: SVG strokes no
/// subpath that is a single moveto.
fn path_to_subpaths(commands: &[PathCommand], transform: Transform) -> Vec<Subpath> {
    let mut subpaths: Vec<Subpath> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    // Whether `current` has had anything drawn in it since its moveto.
    let mut drawn = false;
    let mut cursor_x: f32 = 0.0;
    let mut cursor_y: f32 = 0.0;
    let mut start_x: f32 = 0.0;
    let mut start_y: f32 = 0.0;
    // For smooth curves, we track the last control point
    let mut last_cubic_cp: Option<(f32, f32)> = None;
    let mut last_quad_cp: Option<(f32, f32)> = None;
    let scale = transform.length_scale();

    for cmd in commands {
        // Drawing after a closepath with no moveto between starts a new run
        // from the closed run's start, which is where the pen is.
        if current.is_empty() && !matches!(cmd, PathCommand::MoveTo { .. }) {
            current.push(transform.apply(start_x, start_y));
            drawn = false;
        }
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if drawn {
                    subpaths.push(Subpath {
                        points: core::mem::take(&mut current),
                        closed: false,
                    });
                }
                current.clear();
                drawn = false;
                current.push(transform.apply(*x, *y));
                cursor_x = *x;
                cursor_y = *y;
                start_x = *x;
                start_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::LineTo { x, y } => {
                current.push(transform.apply(*x, *y));
                drawn = true;
                cursor_x = *x;
                cursor_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::HorizontalLineTo { x } => {
                current.push(transform.apply(*x, cursor_y));
                drawn = true;
                cursor_x = *x;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::VerticalLineTo { y } => {
                current.push(transform.apply(cursor_x, *y));
                drawn = true;
                cursor_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::CubicBezier {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (tx0, ty0) = transform.apply(cursor_x, cursor_y);
                let (tx1, ty1) = transform.apply(*x1, *y1);
                let (tx2, ty2) = transform.apply(*x2, *y2);
                let (tx3, ty3) = transform.apply(*x, *y);
                flatten_cubic(
                    tx0,
                    ty0,
                    tx1,
                    ty1,
                    tx2,
                    ty2,
                    tx3,
                    ty3,
                    DEFAULT_FLATNESS,
                    &mut current,
                );
                drawn = true;
                last_cubic_cp = Some((*x2, *y2));
                last_quad_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::SmoothCubic { x2, y2, x, y } => {
                // Reflected control point
                let (rx1, ry1) = match last_cubic_cp {
                    Some((lx, ly)) => (2.0 * cursor_x - lx, 2.0 * cursor_y - ly),
                    None => (cursor_x, cursor_y),
                };
                let (tx0, ty0) = transform.apply(cursor_x, cursor_y);
                let (tx1, ty1) = transform.apply(rx1, ry1);
                let (tx2, ty2) = transform.apply(*x2, *y2);
                let (tx3, ty3) = transform.apply(*x, *y);
                flatten_cubic(
                    tx0,
                    ty0,
                    tx1,
                    ty1,
                    tx2,
                    ty2,
                    tx3,
                    ty3,
                    DEFAULT_FLATNESS,
                    &mut current,
                );
                drawn = true;
                last_cubic_cp = Some((*x2, *y2));
                last_quad_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::QuadraticBezier { x1, y1, x, y } => {
                let (tx0, ty0) = transform.apply(cursor_x, cursor_y);
                let (tx1, ty1) = transform.apply(*x1, *y1);
                let (tx2, ty2) = transform.apply(*x, *y);
                flatten_quadratic(tx0, ty0, tx1, ty1, tx2, ty2, DEFAULT_FLATNESS, &mut current);
                drawn = true;
                last_quad_cp = Some((*x1, *y1));
                last_cubic_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::SmoothQuadratic { x, y } => {
                let (rx1, ry1) = match last_quad_cp {
                    Some((lx, ly)) => (2.0 * cursor_x - lx, 2.0 * cursor_y - ly),
                    None => (cursor_x, cursor_y),
                };
                let (tx0, ty0) = transform.apply(cursor_x, cursor_y);
                let (tx1, ty1) = transform.apply(rx1, ry1);
                let (tx2, ty2) = transform.apply(*x, *y);
                flatten_quadratic(tx0, ty0, tx1, ty1, tx2, ty2, DEFAULT_FLATNESS, &mut current);
                drawn = true;
                last_quad_cp = Some((rx1, ry1));
                last_cubic_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::Arc {
                rx,
                ry,
                x_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                // Flattened in user space and then carried to device space;
                // `scale` is what lets it cut as many segments as the curve
                // will be drawn large.
                let mut arc_pts = Vec::new();
                flatten_arc(
                    cursor_x,
                    cursor_y,
                    *rx,
                    *ry,
                    *x_rotation,
                    *large_arc,
                    *sweep,
                    *x,
                    *y,
                    scale,
                    &mut arc_pts,
                );
                for (px, py) in &arc_pts {
                    current.push(transform.apply(*px, *py));
                }
                drawn = true;
                last_cubic_cp = None;
                last_quad_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::Close => {
                // Closed runs are closed by the flag, not by a repeated point:
                // the stroke joins the ends, and the fill closes every run.
                subpaths.push(Subpath {
                    points: core::mem::take(&mut current),
                    closed: true,
                });
                drawn = false;
                cursor_x = start_x;
                cursor_y = start_y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
        }
    }

    if drawn {
        subpaths.push(Subpath {
            points: current,
            closed: false,
        });
    }

    subpaths
}

// ─── Scanline Rasterizer ─────────────────────────────────────────────────────

/// One edge of a shape being filled, running down the page.
struct FillEdge {
    /// Where it starts: its topmost end.
    x_top: f32,
    y_top: f32,
    /// Where it stops, exclusive: a scanline through its bottom end is not
    /// crossed by it, which is what counts a shared vertex once.
    y_bottom: f32,
    /// How far it moves right per pixel down.
    slope: f32,
    /// +1 where the outline ran downwards here, -1 where it ran up.
    winding: i32,
}

/// Software rasterizer that renders SVG to a pixel buffer.
struct SvgRenderer {
    width: u32,
    height: u32,
    /// 4 bytes per pixel, `[r, g, b, a]`, straight alpha, row by row -- what
    /// `blend_pixel` writes.
    buffer: Vec<u8>,
    /// Sub-scanlines per pixel row, for anti-aliasing vertically; coverage
    /// across a row is measured exactly.
    ss_factor: u32,
}

impl SvgRenderer {
    fn new(width: u32, height: u32) -> Self {
        // A size that does not fit in `usize` could not be allocated even if it
        // were computed, so an empty buffer is the honest answer rather than a
        // wrapped one. Every write goes through `pixel_mut`, which checks the
        // buffer's real length, so a short buffer renders blank instead of out
        // of bounds.
        let size = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(w, h)| w.checked_mul(h))
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(0);
        Self {
            width,
            height,
            buffer: vec![0u8; size],
            ss_factor: 4,
        }
    }

    fn render_node(&mut self, node: &SvgNode, transform: Transform, parent_style: &ResolvedStyle) {
        match node {
            SvgNode::Svg { children, .. } => {
                for child in children {
                    self.render_node(child, transform, parent_style);
                }
            }
            SvgNode::Group {
                transform: local_xf,
                style,
                children,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                for child in children {
                    self.render_node(child, combined, &resolved);
                }
            }
            SvgNode::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let outline = rect_subpath(*x, *y, *width, *height, *rx, *ry, combined);
                self.paint(outline.as_slice(), &resolved, combined);
            }
            SvgNode::Circle {
                cx,
                cy,
                r,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let outline = ellipse_subpath(*cx, *cy, *r, *r, combined);
                self.paint(outline.as_slice(), &resolved, combined);
            }
            SvgNode::Ellipse {
                cx,
                cy,
                rx,
                ry,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let outline = ellipse_subpath(*cx, *cy, *rx, *ry, combined);
                self.paint(outline.as_slice(), &resolved, combined);
            }
            SvgNode::Line {
                x1,
                y1,
                x2,
                y2,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let outline = points_subpath(&[(*x1, *y1), (*x2, *y2)], combined, false);
                self.paint(&[outline], &resolved, combined);
            }
            SvgNode::Polyline {
                points,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                // A polyline is open: no closing segment.
                let outline = points_subpath(points, combined, false);
                self.paint(&[outline], &resolved, combined);
            }
            SvgNode::Polygon {
                points,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let outline = points_subpath(points, combined, true);
                self.paint(&[outline], &resolved, combined);
            }
            SvgNode::Path {
                commands,
                transform: local_xf,
                style,
            } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let subpaths = path_to_subpaths(commands, combined);
                self.paint(&subpaths, &resolved, combined);
            }
        }
    }

    /// Fill and stroke one shape, `subpaths` in device space, as `style` says.
    ///
    /// The fill first, as SVG paints them. `transform` is the one the shape
    /// was carried to device space by: a stroke's width is a length in user
    /// space, so it grows and shrinks with the drawing. It was drawn in device
    /// pixels, so every icon's lines were two pixels thick at every size --
    /// too heavy at 16 and hairlines at 64.
    fn paint(&mut self, subpaths: &[Subpath], style: &ResolvedStyle, transform: Transform) {
        if subpaths.is_empty() {
            return;
        }
        if let Some(fill) = style.effective_fill_color() {
            let outlines: Vec<&[(f32, f32)]> =
                subpaths.iter().map(|s| s.points.as_slice()).collect();
            self.fill_shape(&outlines, style.fill_rule, fill);
        }
        if let Some(stroke) = style.effective_stroke_color() {
            let geometry = StrokeGeometry {
                width: style.stroke_width * transform.length_scale(),
                cap: style.line_cap,
                join: style.line_join,
                miter_limit: style.miter_limit,
            };
            let polygons = stroke_polygons(subpaths, geometry);
            let outlines: Vec<&[(f32, f32)]> = polygons.iter().map(Vec::as_slice).collect();
            self.fill_shape(&outlines, FillRule::NonZero, stroke);
        }
    }

    /// Fill the shape `outlines` bound -- each closed back to its start -- as
    /// one shape under `rule`, and blend it into the buffer once.
    ///
    /// One shape, not one polygon at a time: every edge of every outline takes
    /// part in one winding count, so an outline inside another is a hole where
    /// the rule makes it one, and a pixel two outlines overlap is covered once
    /// rather than blended twice. Each sub-scanline's crossings are found from
    /// the edges it passes through, which a list sorted by top keeps short.
    fn fill_shape(&mut self, outlines: &[&[(f32, f32)]], rule: FillRule, color: Color) {
        if self.buffer.is_empty() || color.a == 0 {
            return;
        }
        let mut edges: Vec<FillEdge> = Vec::new();
        let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
        for outline in outlines {
            let closing = match outline {
                [first, .., last] => Some((*last, *first)),
                _ => None,
            };
            let sides = outline
                .windows(2)
                .filter_map(|w| match *w {
                    [a, b] => Some((a, b)),
                    _ => None,
                })
                .chain(closing);
            for (a, b) in sides {
                if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
                    continue;
                }
                let (top, bottom, winding) = match a.1.partial_cmp(&b.1) {
                    Some(core::cmp::Ordering::Less) => (a, b, 1),
                    Some(core::cmp::Ordering::Greater) => (b, a, -1),
                    // Level: crosses no scanline.
                    _ => continue,
                };
                min_x = min_x.min(a.0).min(b.0);
                max_x = max_x.max(a.0).max(b.0);
                min_y = min_y.min(top.1);
                max_y = max_y.max(bottom.1);
                edges.push(FillEdge {
                    x_top: top.0,
                    y_top: top.1,
                    y_bottom: bottom.1,
                    slope: (bottom.0 - top.0) / (bottom.1 - top.1),
                    winding,
                });
            }
        }
        if edges.is_empty() {
            return;
        }
        edges.sort_by(|a, b| a.y_top.total_cmp(&b.y_top));

        // Rows and columns the shape can reach, on the surface.
        let rows = (min_y.floor().max(0.0) as u32)..(max_y.ceil().min(self.height as f32) as u32);
        let first_col = min_x.floor().max(0.0) as u32;
        let end_col = max_x.ceil().min(self.width as f32) as u32;
        let Ok(columns) = usize::try_from(end_col.saturating_sub(first_col)) else {
            return;
        };
        if rows.is_empty() || columns == 0 {
            return;
        }
        let origin = first_col as f32;
        let ss = self.ss_factor.max(1);
        let weight = 1.0 / ss as f32;

        let mut coverage = vec![0.0f32; columns];
        let mut active: Vec<usize> = Vec::new();
        let mut next_edge = 0usize;
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        for row in rows {
            coverage.fill(0.0);
            for sub in 0..ss {
                let scan_y = row as f32 + (sub as f32 + 0.5) * weight;
                // Edges that have begun by this line join the active list...
                while let Some(edge) = edges.get(next_edge) {
                    if edge.y_top > scan_y {
                        break;
                    }
                    active.push(next_edge);
                    next_edge = next_edge.saturating_add(1);
                }
                // ...and those that have ended leave it.
                active.retain(|&i| edges.get(i).is_some_and(|e| e.y_bottom > scan_y));
                crossings.clear();
                crossings.extend(active.iter().filter_map(|&i| {
                    edges
                        .get(i)
                        .map(|e| (e.x_top + (scan_y - e.y_top) * e.slope, e.winding))
                }));
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut winding = 0i32;
                for pair in crossings.windows(2) {
                    let &[(left, turn), (right, _)] = pair else {
                        continue;
                    };
                    winding = winding.saturating_add(turn);
                    let inside = match rule {
                        FillRule::NonZero => winding != 0,
                        FillRule::EvenOdd => winding & 1 != 0,
                    };
                    if inside {
                        cover_span(&mut coverage, left - origin, right - origin, weight);
                    }
                }
            }
            for (col, &cov) in (first_col..).zip(&coverage) {
                let alpha = (cov.min(1.0) * f32::from(color.a)).round() as u8;
                if alpha > 0 {
                    self.blend_pixel(col, row, Color::rgba(color.r, color.g, color.b, alpha));
                }
            }
        }
    }

    /// The four bytes of one pixel, or `None` if it lies outside the surface.
    ///
    /// The buffer is `[R, G, B, A]` per pixel, and every access used to compute
    /// an offset and then index four times at `offset`, `+1`, `+2`, `+3` —
    /// eight indexes behind a single bounds test on the last of them. Returning
    /// the four bytes as an array makes the test and the reads one operation
    /// and puts the arity in the type, so a five-byte format could not be half
    /// introduced.
    fn pixel_mut(&mut self, x: u32, y: u32) -> Option<&mut [u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        // Checked rather than plain arithmetic: on a surface large enough for
        // `width * height * 4` to exceed `u32`, wrapping would name a *valid*
        // offset belonging to some other pixel, which is a silently corrupted
        // image rather than a caught error.
        let offset = y
            .checked_mul(self.width)
            .and_then(|row| row.checked_add(x))
            .and_then(|index| index.checked_mul(4))
            .and_then(|byte| usize::try_from(byte).ok())?;
        let end = offset.checked_add(4)?;
        self.buffer.get_mut(offset..end)?.try_into().ok()
    }

    /// Blend a single pixel (alpha compositing).
    fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        let Some(pixel) = self.pixel_mut(x, y) else {
            return;
        };
        let [r, g, b, a] = *pixel;
        let result = color.over(Color::rgba(r, g, b, a));
        *pixel = [result.r, result.g, result.b, result.a];
    }
}

/// Add `weight` times the share of each pixel the span `left..right` covers to
/// `coverage`, whose first entry is the pixel whose left edge is at 0.
fn cover_span(coverage: &mut [f32], left: f32, right: f32, weight: f32) {
    let left = left.max(0.0);
    let right = right.min(coverage.len() as f32);
    if left.is_nan() || right.is_nan() || right <= left {
        return;
    }
    let first = left.floor() as usize;
    let end = (right.ceil() as usize).min(coverage.len());
    let Some(cells) = coverage.get_mut(first..end) else {
        return;
    };
    for (col, cell) in (first..).zip(cells.iter_mut()) {
        let pixel_left = col as f32;
        let covered = right.min(pixel_left + 1.0) - left.max(pixel_left);
        if covered > 0.0 {
            *cell += covered * weight;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn a_transform_with_a_close_paren_before_the_open_one_is_an_error() {
        // `find(')')` used to search the whole string rather than the part
        // after the '(', so this input produced a start index past its end and
        // panicked inside the slice. SVG is a file format — a malformed
        // attribute must be an error, never a crash.
        assert!(parse_transform(")x(1,2)").is_err());
        assert!(parse_transform("translate)1,2(").is_err());
        // The well-formed cases still parse, including several in sequence.
        let t = parse_transform("translate(10,20) scale(2)").unwrap();
        assert_eq!(t.apply(0.0, 0.0), (10.0, 20.0));
        assert_eq!(t.apply(1.0, 1.0), (12.0, 22.0));
    }

    #[test]
    fn a_viewbox_is_four_numbers_and_nothing_else() {
        assert_eq!(
            parse_viewbox("0 0 100 50").unwrap(),
            (0.0, 0.0, 100.0, 50.0)
        );
        assert_eq!(
            parse_viewbox("-1,-2, 3 ,4").unwrap(),
            (-1.0, -2.0, 3.0, 4.0)
        );
        // Too few and too many are both errors: the slice pattern that binds
        // the four values is the same expression that rejects any other count.
        assert!(parse_viewbox("0 0 100").is_err());
        assert!(parse_viewbox("0 0 100 50 7").is_err());
        assert!(parse_viewbox("").is_err());
        assert!(parse_viewbox("0 0 wide tall").is_err());
    }

    #[test]
    fn points_are_read_in_pairs_and_an_odd_count_is_refused() {
        assert_eq!(
            parse_points("1,2 3,4 5 6").unwrap(),
            vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]
        );
        assert_eq!(parse_points("").unwrap(), vec![]);
        assert!(parse_points("1,2 3").is_err());
    }

    #[test]
    fn a_document_with_no_elements_is_an_error_not_a_panic() {
        // The emptiness check and the read of the first element used to be two
        // statements; only the first of them was enforced.
        assert!(SvgDocument::parse("").is_err());
        assert!(SvgDocument::parse("   \n\t ").is_err());
        assert!(SvgDocument::parse("<?xml version=\"1.0\"?>").is_err());
        assert!(SvgDocument::parse("<!-- just a comment -->").is_err());
    }

    #[test]
    fn a_curve_that_never_flattens_still_terminates() {
        // Coincident control points make the flatness test degenerate; the
        // subdivision budget is what stops the recursion, and it is spent
        // downwards so it cannot be outlived.
        let mut out = Vec::new();
        flatten_cubic(0.0, 0.0, 1e30, 1e30, -1e30, -1e30, 0.0, 0.0, 0.0, &mut out);
        assert!(!out.is_empty(), "a flattened curve always ends somewhere");
        // 2^MAX_SUBDIVISIONS chords is the worst case; anything more means the
        // budget stopped bounding the recursion.
        assert!(out.len() <= 1usize << MAX_SUBDIVISIONS);
        assert_eq!(
            out.last().copied(),
            Some((0.0, 0.0)),
            "ends at the endpoint"
        );
    }

    #[test]
    fn a_render_size_that_cannot_be_allocated_yields_no_pixels() {
        // `width * height * 4` overflowing `usize` used to wrap. `render` is
        // public, so the dimensions are the caller's, and a wrapped size is a
        // buffer smaller than the image it claims to be.
        let doc = SvgDocument::parse("<svg width=\"10\" height=\"10\"><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\" fill=\"red\"/></svg>")
            .unwrap();
        let pixels = doc.render(4, 4);
        assert_eq!(pixels.len(), 4 * 4 * 4);
        // A degenerate size is empty rather than wrapped or panicking.
        assert!(doc.render(0, 0).is_empty());
    }

    // --- XML parsing tests ---

    #[test]
    fn test_parse_xml_basic() {
        let svg =
            r#"<svg width="100" height="100"><rect x="10" y="20" width="30" height="40"/></svg>"#;
        let elems = parse_xml(svg).unwrap();
        assert_eq!(elems.len(), 1);
        assert_eq!(elems[0].tag, "svg");
        assert_eq!(elems[0].attr("width"), Some("100"));
        assert_eq!(elems[0].children.len(), 1);
        assert_eq!(elems[0].children[0].tag, "rect");
    }

    #[test]
    fn test_parse_xml_self_closing() {
        let svg = r#"<svg><circle cx="50" cy="50" r="25"/></svg>"#;
        let elems = parse_xml(svg).unwrap();
        assert_eq!(elems[0].children.len(), 1);
        assert_eq!(elems[0].children[0].tag, "circle");
        assert_eq!(elems[0].children[0].attr("r"), Some("25"));
    }

    #[test]
    fn test_parse_xml_nested() {
        let svg = r#"<svg><g transform="translate(10,20)"><rect x="0" y="0" width="5" height="5"/></g></svg>"#;
        let elems = parse_xml(svg).unwrap();
        let g = &elems[0].children[0];
        assert_eq!(g.tag, "g");
        assert_eq!(g.children.len(), 1);
        assert_eq!(g.children[0].tag, "rect");
    }

    #[test]
    fn test_parse_xml_with_prolog() {
        let svg = r#"<?xml version="1.0"?>
        <!-- comment -->
        <svg width="50" height="50"></svg>"#;
        let elems = parse_xml(svg).unwrap();
        assert_eq!(elems[0].tag, "svg");
    }

    // --- Path data parsing tests ---

    #[test]
    fn test_path_moveto_lineto() {
        let cmds = parse_path_data("M 10 20 L 30 40").unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0], PathCommand::MoveTo { x: 10.0, y: 20.0 });
        assert_eq!(cmds[1], PathCommand::LineTo { x: 30.0, y: 40.0 });
    }

    #[test]
    fn test_path_relative() {
        let cmds = parse_path_data("M 10 20 l 5 5").unwrap();
        assert_eq!(cmds[1], PathCommand::LineTo { x: 15.0, y: 25.0 });
    }

    #[test]
    fn test_path_horizontal_vertical() {
        let cmds = parse_path_data("M 0 0 H 10 V 20").unwrap();
        assert_eq!(cmds[1], PathCommand::HorizontalLineTo { x: 10.0 });
        assert_eq!(cmds[2], PathCommand::VerticalLineTo { y: 20.0 });
    }

    #[test]
    fn test_path_cubic_bezier() {
        let cmds = parse_path_data("M 0 0 C 10 20 30 40 50 60").unwrap();
        assert_eq!(
            cmds[1],
            PathCommand::CubicBezier {
                x1: 10.0,
                y1: 20.0,
                x2: 30.0,
                y2: 40.0,
                x: 50.0,
                y: 60.0
            }
        );
    }

    #[test]
    fn test_path_quadratic() {
        let cmds = parse_path_data("M 0 0 Q 10 20 30 40").unwrap();
        assert_eq!(
            cmds[1],
            PathCommand::QuadraticBezier {
                x1: 10.0,
                y1: 20.0,
                x: 30.0,
                y: 40.0
            }
        );
    }

    #[test]
    fn test_path_arc() {
        let cmds = parse_path_data("M 0 0 A 25 25 0 0 1 50 50").unwrap();
        assert_eq!(
            cmds[1],
            PathCommand::Arc {
                rx: 25.0,
                ry: 25.0,
                x_rotation: 0.0,
                large_arc: false,
                sweep: true,
                x: 50.0,
                y: 50.0,
            }
        );
    }

    #[test]
    fn test_path_close() {
        let cmds = parse_path_data("M 0 0 L 10 0 L 10 10 Z").unwrap();
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[3], PathCommand::Close);
    }

    #[test]
    fn test_path_compact_notation() {
        // No spaces between numbers using negative signs as separators
        let cmds = parse_path_data("M10-5L20-10").unwrap();
        assert_eq!(cmds[0], PathCommand::MoveTo { x: 10.0, y: -5.0 });
        assert_eq!(cmds[1], PathCommand::LineTo { x: 20.0, y: -10.0 });
    }

    #[test]
    fn test_path_smooth_cubic() {
        let cmds = parse_path_data("M 0 0 C 10 20 30 40 50 60 S 70 80 90 100").unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(
            cmds[2],
            PathCommand::SmoothCubic {
                x2: 70.0,
                y2: 80.0,
                x: 90.0,
                y: 100.0
            }
        );
    }

    #[test]
    fn test_path_smooth_quadratic() {
        let cmds = parse_path_data("M 0 0 Q 10 20 30 30 T 50 50").unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[2], PathCommand::SmoothQuadratic { x: 50.0, y: 50.0 });
    }

    // --- Color parsing tests ---

    #[test]
    fn test_color_hex_short() {
        let c = parse_color("#f00").unwrap();
        assert_eq!(c, SvgPaint::Color(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn test_color_hex_long() {
        let c = parse_color("#ff8000").unwrap();
        assert_eq!(c, SvgPaint::Color(Color::rgb(255, 128, 0)));
    }

    #[test]
    fn test_color_named() {
        assert_eq!(
            parse_color("red").unwrap(),
            SvgPaint::Color(Color::rgb(255, 0, 0))
        );
        assert_eq!(
            parse_color("blue").unwrap(),
            SvgPaint::Color(Color::rgb(0, 0, 255))
        );
        assert_eq!(
            parse_color("green").unwrap(),
            SvgPaint::Color(Color::rgb(0, 128, 0))
        );
    }

    #[test]
    fn test_color_rgb_func() {
        let c = parse_color("rgb(128, 64, 32)").unwrap();
        assert_eq!(c, SvgPaint::Color(Color::rgb(128, 64, 32)));
    }

    #[test]
    fn test_color_rgba_func() {
        let c = parse_color("rgba(255, 128, 0, 0.5)").unwrap();
        assert_eq!(c, SvgPaint::Color(Color::rgba(255, 128, 0, 127)));
    }

    #[test]
    fn test_color_none() {
        assert_eq!(parse_color("none").unwrap(), SvgPaint::None);
    }

    #[test]
    fn test_color_transparent() {
        assert_eq!(
            parse_color("transparent").unwrap(),
            SvgPaint::Color(Color::TRANSPARENT)
        );
    }

    #[test]
    fn test_color_current_color() {
        assert_eq!(parse_color("currentColor").unwrap(), SvgPaint::CurrentColor);
    }

    /// A hex colour is scanned byte-wise, so a multi-byte character in it lands
    /// in the error path one byte at a time.  The message must not name a
    /// character that is not in the input: "ÿ" is 0xC3 0xBF, and reporting the
    /// lead byte as Latin-1 would blame "Ã", sending the author looking for a
    /// character they never typed.
    #[test]
    fn a_multibyte_char_in_a_hex_colour_is_not_reported_as_latin1() {
        let err = parse_color("#ÿÿÿ").expect_err("not a hex colour");
        let SvgError::InvalidColor(msg) = err else {
            panic!("expected InvalidColor, got {err:?}");
        };
        assert!(
            !msg.contains('Ã'),
            "error names a character absent from the input: {msg}"
        );
        assert!(msg.contains("0xc3"), "error should name the byte: {msg}");
    }

    /// The ASCII case still reads naturally.
    #[test]
    fn a_bad_ascii_hex_digit_is_reported_as_itself() {
        let err = parse_color("#zzz").expect_err("not a hex colour");
        let SvgError::InvalidColor(msg) = err else {
            panic!("expected InvalidColor, got {err:?}");
        };
        assert!(msg.contains('z'), "{msg}");
    }

    // --- Transform parsing tests ---

    #[test]
    fn test_transform_translate() {
        let t = parse_transform("translate(10, 20)").unwrap();
        assert!((t.tx - 10.0).abs() < 1e-5);
        assert!((t.ty - 20.0).abs() < 1e-5);
    }

    #[test]
    fn test_transform_scale() {
        let t = parse_transform("scale(2, 3)").unwrap();
        assert!((t.a - 2.0).abs() < 1e-5);
        assert!((t.d - 3.0).abs() < 1e-5);
    }

    #[test]
    fn test_transform_rotate() {
        let t = parse_transform("rotate(90)").unwrap();
        // 90 degrees: cos=0, sin=1 => a=0, b=1, c=-1, d=0
        assert!(t.a.abs() < 1e-5);
        assert!((t.b - 1.0).abs() < 1e-5);
        assert!((t.c + 1.0).abs() < 1e-5);
        assert!(t.d.abs() < 1e-5);
    }

    #[test]
    fn test_transform_matrix() {
        let t = parse_transform("matrix(1, 0, 0, 1, 50, 60)").unwrap();
        assert!((t.a - 1.0).abs() < 1e-5);
        assert!((t.tx - 50.0).abs() < 1e-5);
        assert!((t.ty - 60.0).abs() < 1e-5);
    }

    #[test]
    fn test_transform_chained() {
        let t = parse_transform("translate(10, 0) scale(2)").unwrap();
        // translate(10,0) then scale(2): result should map (0,0) to (10,0) scaled by 2 = (20,0)?
        // Actually: combined = translate.then(scale) = first apply scale, then translate
        // apply(x,y): scale -> (2x, 2y), then translate -> (2x+10, 2y+0)
        // Wait, our `.then()` is self * other, meaning apply other first then self.
        // Result of "translate(10,0) scale(2)" means first translate comes, then scale.
        // In SVG, transforms are applied right-to-left: scale first, then translate.
        let (x, y) = t.apply(5.0, 0.0);
        // scale(2): 5 -> 10, then translate(10): 10+10 = 20
        assert!((x - 20.0).abs() < 1e-4);
        assert!(y.abs() < 1e-4);
    }

    // --- Bézier flattening tests ---

    #[test]
    fn test_flatten_straight_line() {
        // A "cubic" that's actually a straight line: all control points collinear
        let mut output = Vec::new();
        flatten_cubic(
            0.0,
            0.0,
            10.0,
            10.0,
            20.0,
            20.0,
            30.0,
            30.0,
            0.25,
            &mut output,
        );
        // Should be very few points since it's flat
        assert!(!output.is_empty());
        assert!(output.len() <= 4); // Should be just 1 point (the end)
    }

    #[test]
    fn test_flatten_curve_produces_points() {
        // A real curve should produce more points
        let mut output = Vec::new();
        flatten_cubic(
            0.0,
            0.0,
            0.0,
            100.0,
            100.0,
            100.0,
            100.0,
            0.0,
            0.25,
            &mut output,
        );
        // This is a pronounced S-curve that needs many segments
        assert!(output.len() > 4);
    }

    #[test]
    fn test_flatten_quadratic_produces_points() {
        let mut output = Vec::new();
        flatten_quadratic(0.0, 0.0, 50.0, 100.0, 100.0, 0.0, 0.25, &mut output);
        assert!(output.len() > 2);
    }

    // --- ViewBox scaling tests ---

    #[test]
    fn test_viewbox_parsing() {
        let doc =
            SvgDocument::parse(r#"<svg viewBox="0 0 100 100" width="200" height="200"></svg>"#)
                .unwrap();
        assert_eq!(doc.viewbox(), (0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn test_viewbox_default() {
        let doc = SvgDocument::parse(r#"<svg width="300" height="150"></svg>"#).unwrap();
        assert_eq!(doc.viewbox(), (0.0, 0.0, 300.0, 150.0));
    }

    #[test]
    fn test_viewbox_offset() {
        let doc = SvgDocument::parse(r#"<svg viewBox="10 20 80 60"></svg>"#).unwrap();
        assert_eq!(doc.viewbox(), (10.0, 20.0, 80.0, 60.0));
    }

    // --- Document tree tests ---

    #[test]
    fn test_document_rect() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100"><rect x="10" y="10" width="80" height="80" fill="red"/></svg>"#,
        ).unwrap();
        match &doc.root {
            SvgNode::Svg { children, .. } => {
                assert_eq!(children.len(), 1);
                match &children[0] {
                    SvgNode::Rect {
                        x,
                        y,
                        width,
                        height,
                        style,
                        ..
                    } => {
                        assert!((x - 10.0).abs() < 1e-5);
                        assert!((y - 10.0).abs() < 1e-5);
                        assert!((width - 80.0).abs() < 1e-5);
                        assert!((height - 80.0).abs() < 1e-5);
                        assert_eq!(style.fill, Some(SvgPaint::Color(Color::rgb(255, 0, 0))));
                    }
                    _ => panic!("expected Rect node"),
                }
            }
            _ => panic!("expected Svg root"),
        }
    }

    #[test]
    fn test_document_group() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100">
                <g fill="blue" transform="translate(5, 5)">
                    <circle cx="50" cy="50" r="25"/>
                </g>
            </svg>"#,
        )
        .unwrap();
        match &doc.root {
            SvgNode::Svg { children, .. } => match &children[0] {
                SvgNode::Group {
                    transform,
                    style,
                    children,
                } => {
                    assert!((transform.tx - 5.0).abs() < 1e-5);
                    assert!((transform.ty - 5.0).abs() < 1e-5);
                    assert_eq!(style.fill, Some(SvgPaint::Color(Color::rgb(0, 0, 255))));
                    assert_eq!(children.len(), 1);
                }
                _ => panic!("expected Group node"),
            },
            _ => panic!("expected Svg root"),
        }
    }

    // --- Render tests ---

    #[test]
    fn test_render_basic_rect() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 10 10"><rect x="0" y="0" width="10" height="10" fill="red"/></svg>"#,
        ).unwrap();
        let buf = doc.render(10, 10);
        // Buffer should be 10*10*4 = 400 bytes
        assert_eq!(buf.len(), 400);
        // Center pixel should be red
        let center = (5 * 10 + 5) * 4;
        assert_eq!(buf[center], 255); // R
        assert_eq!(buf[center + 1], 0); // G
        assert_eq!(buf[center + 2], 0); // B
        assert_eq!(buf[center + 3], 255); // A
    }

    #[test]
    fn test_render_circle_center_filled() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="40" fill="blue"/></svg>"#,
        )
        .unwrap();
        let buf = doc.render(100, 100);
        // Center should be filled with blue
        let center = (50 * 100 + 50) * 4;
        assert_eq!(buf[center], 0); // R
        assert_eq!(buf[center + 1], 0); // G
        assert_eq!(buf[center + 2], 255); // B
        assert!(buf[center + 3] > 200); // A (should be full or near-full)
    }

    // --- rasterizing: strokes and fills ---

    /// The alpha of pixel `(x, y)` in a `size`-square render of `svg`.
    fn alpha_at(buf: &[u8], size: usize, x: usize, y: usize) -> u8 {
        buf[(y * size + x) * 4 + 3]
    }

    /// How many pixels of column `x` are drawn at all.
    fn inked_in_column(buf: &[u8], size: usize, x: usize) -> usize {
        (0..size).filter(|&y| alpha_at(buf, size, x, y) > 0).count()
    }

    /// **A stroke's width is a length in the drawing**, so it grows with the
    /// size the drawing is rendered at. It was drawn in device pixels: two
    /// units were two pixels at every size, too heavy small and a hairline
    /// large.
    #[test]
    fn a_stroke_is_as_wide_as_the_drawing_is_large() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 10 10"><line x1="0" y1="5" x2="10" y2="5" stroke="black" stroke-width="2"/></svg>"#,
        )
        .unwrap();
        assert_eq!(inked_in_column(&doc.render(10, 10), 10, 5), 2);
        assert_eq!(inked_in_column(&doc.render(40, 40), 40, 20), 8);
    }

    /// **A curved stroke is solid where it covers a pixel.** Drawn a quad per
    /// segment, each blended on its own, the short segments of a curve each
    /// covered part of a pixel and none made it solid: a ring came out at half
    /// strength.
    #[test]
    fn a_curved_stroke_is_solid_where_it_covers_a_pixel() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 40 40"><circle cx="20" cy="20" r="12" fill="none" stroke="black" stroke-width="4"/></svg>"#,
        )
        .unwrap();
        let buf = doc.render(40, 40);
        // The ring's middle, left and right, top and bottom.
        for (x, y) in [(32, 20), (7, 20), (20, 32), (20, 7)] {
            assert_eq!(alpha_at(&buf, 40, x, y), 255, "at ({x}, {y})");
        }
        // And its hole and the outside are clear.
        assert_eq!(alpha_at(&buf, 40, 20, 20), 0);
        assert_eq!(alpha_at(&buf, 40, 1, 1), 0);
    }

    /// **A translucent stroke is one shade across its joints.** Blended quad by
    /// quad, the pixels two segments shared were blended twice and darkened at
    /// every corner of the line.
    #[test]
    fn a_translucent_stroke_is_one_shade_across_its_joints() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 40 40"><polyline points="4,30 20,8 36,30" fill="none" stroke="rgba(0,0,0,0.5)" stroke-width="4" stroke-linejoin="round"/></svg>"#,
        )
        .unwrap();
        let buf = doc.render(40, 40);
        let SvgPaint::Color(ink) = parse_color("rgba(0,0,0,0.5)").unwrap() else {
            panic!("not a colour");
        };
        let most = (0..40 * 40).map(|i| buf[i * 4 + 3]).max().unwrap();
        assert!(most <= ink.a, "a pixel was covered twice: alpha {most}");
        // Just inside the corner, where the two segments and the join all
        // cover: drawn once, at the stroke's own strength.
        assert_eq!(alpha_at(&buf, 40, 20, 9), ink.a);
    }

    /// **A subpath inside another is a hole where the fill rule says so.**
    /// Every subpath was filled as a shape of its own, so a ring drawn as two
    /// circles was a solid disc under any rule.
    #[test]
    fn a_subpath_inside_another_is_a_hole_as_the_fill_rule_says() {
        let render = |rule: &str, inner: &str| {
            let svg = format!(
                r#"<svg viewBox="0 0 10 10"><path fill-rule="{rule}" d="M0 0H10V10H0Z {inner}"/></svg>"#
            );
            SvgDocument::parse(&svg).unwrap().render(10, 10)
        };
        let same_way = "M3 3H7V7H3Z";
        let other_way = "M3 3V7H7V3Z";
        // Even-odd: a hole whichever way the inner square is drawn.
        assert_eq!(alpha_at(&render("evenodd", same_way), 10, 5, 5), 0);
        assert_eq!(alpha_at(&render("evenodd", other_way), 10, 5, 5), 0);
        // Nonzero, SVG's default: a hole only when drawn the other way round.
        assert_eq!(alpha_at(&render("nonzero", same_way), 10, 5, 5), 255);
        assert_eq!(alpha_at(&render("nonzero", other_way), 10, 5, 5), 0);
        // The outer square is filled in every case.
        assert_eq!(alpha_at(&render("evenodd", same_way), 10, 1, 1), 255);
    }

    /// **The three line caps**: butt stops at the end point, square carries on
    /// half the width, round adds a half disc.
    #[test]
    fn the_line_cap_shapes_the_open_ends() {
        let render = |cap: &str| {
            let svg = format!(
                r#"<svg viewBox="0 0 20 20"><line x1="6" y1="10" x2="14" y2="10" stroke="black" stroke-width="8" stroke-linecap="{cap}"/></svg>"#
            );
            SvgDocument::parse(&svg).unwrap().render(20, 20)
        };
        let butt = render("butt");
        let square = render("square");
        let round = render("round");
        // Just past the start, on the line.
        assert_eq!(alpha_at(&butt, 20, 4, 10), 0);
        assert_eq!(alpha_at(&square, 20, 4, 10), 255);
        assert_eq!(alpha_at(&round, 20, 4, 10), 255);
        // Past the start and off to the side: the square's corner, not the disc.
        assert_eq!(alpha_at(&square, 20, 2, 6), 255);
        assert_eq!(alpha_at(&round, 20, 2, 6), 0);
    }

    /// **The three line joins**, at a right angle: a miter fills the corner
    /// square, a bevel cuts it off, a round one rounds it.
    #[test]
    fn the_line_join_shapes_the_corners() {
        let render = |join: &str, limit: &str| {
            let svg = format!(
                r#"<svg viewBox="0 0 20 20"><polyline points="4,6 14,6 14,16" fill="none" stroke="black" stroke-width="8" stroke-linejoin="{join}" stroke-miterlimit="{limit}"/></svg>"#
            );
            SvgDocument::parse(&svg).unwrap().render(20, 20)
        };
        // The outer corner of the turn, toward (18, 2): inside a miter only.
        assert_eq!(alpha_at(&render("miter", "4"), 20, 17, 2), 255);
        assert_eq!(alpha_at(&render("bevel", "4"), 20, 17, 2), 0);
        assert_eq!(alpha_at(&render("round", "4"), 20, 17, 2), 0);
        // A right angle's miter is sqrt(2) the width: a limit under that bevels.
        assert_eq!(alpha_at(&render("miter", "1.2"), 20, 17, 2), 0);
        // Every join covers the corner's inside.
        for join in ["miter", "bevel", "round"] {
            assert_eq!(alpha_at(&render(join, "4"), 20, 14, 6), 255, "{join}");
        }
    }

    /// **`rx` alone rounds both ways**, as SVG has it. The missing `ry` was
    /// read as 0, which left every such rectangle square.
    #[test]
    fn a_rect_with_rx_alone_is_rounded() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 20 20"><rect x="0" y="0" width="20" height="20" rx="6"/></svg>"#,
        )
        .unwrap();
        let buf = doc.render(20, 20);
        assert_eq!(alpha_at(&buf, 20, 0, 0), 0, "the corner is square");
        assert_eq!(alpha_at(&buf, 20, 10, 10), 255);
    }

    /// **Drawing after a closepath carries on from the closed run's start.**
    /// The run begun after `Z` lost its first point, so its first segment was
    /// never drawn.
    #[test]
    fn drawing_after_a_closepath_starts_where_the_run_began() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 20 20"><path d="M4 4H16V10Z L4 16" fill="none" stroke="black" stroke-width="2"/></svg>"#,
        )
        .unwrap();
        let buf = doc.render(20, 20);
        // The segment from (4, 4) down to (4, 16).
        assert_eq!(alpha_at(&buf, 20, 4, 13), 255);
    }

    /// **A run with no length is its caps**: a dot for a round cap, nothing for
    /// a butt one -- and a moveto alone is not a run.
    #[test]
    fn a_run_with_no_length_is_drawn_as_its_caps() {
        let render = |d: &str, cap: &str| {
            let svg = format!(
                r#"<svg viewBox="0 0 20 20"><path d="{d}" stroke="black" stroke-width="6" stroke-linecap="{cap}"/></svg>"#
            );
            SvgDocument::parse(&svg).unwrap().render(20, 20)
        };
        assert_eq!(alpha_at(&render("M10 10L10 10", "round"), 20, 10, 10), 255);
        assert_eq!(alpha_at(&render("M10 10L10 10", "square"), 20, 8, 8), 255);
        assert_eq!(alpha_at(&render("M10 10L10 10", "butt"), 20, 10, 10), 0);
        assert_eq!(alpha_at(&render("M10 10", "round"), 20, 10, 10), 0);
    }

    /// **An arc is cut as finely as it is drawn large.** It was a fixed eight
    /// segments to the turn, which a large icon showed as corners.
    #[test]
    fn an_arc_is_cut_as_finely_as_it_is_drawn() {
        let cut = |scale: f32| {
            let mut out = Vec::new();
            flatten_arc(
                0.0, 10.0, 10.0, 10.0, 0.0, false, true, 20.0, 10.0, scale, &mut out,
            );
            out
        };
        let small = cut(1.0);
        let large = cut(40.0);
        assert!(
            large.len() > small.len() * 4,
            "{} vs {}",
            large.len(),
            small.len()
        );
        // Each chord at the large size strays at most the tolerance: the
        // midpoint of every chord is within it of the circle.
        let mut previous = (0.0f32, 10.0f32);
        for point in large {
            let mid = (
                f32::midpoint(previous.0, point.0),
                f32::midpoint(previous.1, point.1),
            );
            let off = 10.0 - (mid.0 - 10.0).hypot(mid.1 - 10.0);
            assert!(
                off * 40.0 <= CURVE_TOLERANCE_PX * 1.01,
                "strays {} px",
                off * 40.0
            );
            previous = point;
        }
    }

    /// Stroking hostile geometry draws nothing rather than failing: infinite
    /// and NaN points are dropped, a NaN width is no stroke.
    #[test]
    fn stroking_numbers_that_are_not_numbers_draws_nothing() {
        let run = Subpath {
            points: vec![(f32::NAN, 0.0), (f32::INFINITY, 1.0), (0.0, f32::NAN)],
            closed: false,
        };
        let geometry = StrokeGeometry {
            width: 2.0,
            cap: LineCap::Round,
            join: LineJoin::Miter,
            miter_limit: 4.0,
        };
        assert!(stroke_polygons(std::slice::from_ref(&run), geometry).is_empty());
        let no_width = StrokeGeometry {
            width: f32::NAN,
            ..geometry
        };
        let fine = Subpath {
            points: vec![(0.0, 0.0), (5.0, 0.0)],
            closed: false,
        };
        assert!(stroke_polygons(&[fine], no_width).is_empty());
    }

    /// Every polygon of a stroke winds the same way, which is what lets the
    /// nonzero rule fill their union.
    #[test]
    fn every_polygon_of_a_stroke_winds_the_same_way() {
        let run = Subpath {
            points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            closed: true,
        };
        for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
            let polygons = stroke_polygons(
                std::slice::from_ref(&run),
                StrokeGeometry {
                    width: 2.0,
                    cap: LineCap::Butt,
                    join,
                    miter_limit: 4.0,
                },
            );
            assert!(!polygons.is_empty());
            for polygon in &polygons {
                assert!(signed_area(polygon) <= 0.0, "{join:?}: {polygon:?}");
            }
        }
    }

    /// **A property in the `style` attribute is read, and wins over the
    /// attribute of the same name** -- how Inkscape writes every property,
    /// and so most icon sets drawn with it.
    #[test]
    fn a_property_in_the_style_attribute_is_read_and_wins() {
        let doc = SvgDocument::parse(
            r##"<svg viewBox="0 0 10 10"><rect x="2" y="2" width="6" height="6" fill="#ff0000" style="fill:none; stroke: #00ff00 !important; stroke-width:2px"/></svg>"##,
        )
        .unwrap();
        let px = doc.render(10, 10);
        let at = |x: usize, y: usize| &px[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
        assert_eq!(at(5, 5)[3], 0, "the style's fill:none lost to fill=red");
        let edge = at(2, 5);
        assert!(edge[1] > 200 && edge[0] < 50 && edge[3] == 255, "{edge:?}");

        // A later declaration overrides an earlier one, as in any CSS.
        let later = SvgDocument::parse(
            r#"<svg viewBox="0 0 4 4"><rect width="4" height="4" style="fill:red; fill:blue"/></svg>"#,
        )
        .unwrap();
        assert_eq!(&later.render(4, 4)[..4], &[0, 0, 255, 255]);
    }

    /// A declaration the renderer cannot read is ignored, as a browser
    /// ignores it -- the property inherits -- while an attribute it cannot
    /// read is still an error.
    #[test]
    fn an_unreadable_style_declaration_is_ignored() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 10 10" fill="blue"><rect width="10" height="10" style="fill:url(#g)"/></svg>"#,
        )
        .unwrap();
        assert_eq!(&doc.render(10, 10)[..4], &[0, 0, 255, 255]);
        assert!(
            SvgDocument::parse(
                r#"<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="url(#g)"/></svg>"#
            )
            .is_err()
        );
    }

    /// **Definitions are not drawn where they stand**, and neither is
    /// anything set `display: none`.
    #[test]
    fn definitions_and_hidden_elements_are_not_drawn() {
        for svg in [
            r#"<svg viewBox="0 0 10 10"><defs><rect width="10" height="10"/></defs></svg>"#,
            r#"<svg viewBox="0 0 10 10"><symbol id="s"><rect width="10" height="10"/></symbol></svg>"#,
            r#"<svg viewBox="0 0 10 10"><rect width="10" height="10" display="none"/></svg>"#,
            r#"<svg viewBox="0 0 10 10"><g style="display:none"><rect width="10" height="10"/></g></svg>"#,
            r#"<svg viewBox="0 0 10 10"><style>.a { fill: red; }</style></svg>"#,
        ] {
            let px = SvgDocument::parse(svg).unwrap().render(10, 10);
            assert!(px.iter().all(|b| *b == 0), "{svg} drew something");
        }
    }

    /// Presentation attributes on the root `<svg>` are inherited, as they are
    /// on a `<g>`: an outline icon written `fill="none" stroke="..."` on the
    /// root draws its outline, not a solid black shape.
    #[test]
    fn root_presentation_attributes_are_inherited() {
        let doc = SvgDocument::parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="#00ff00" stroke-width="2"><rect x="2" y="2" width="6" height="6"/></svg>"##,
        )
        .unwrap();
        let px = doc.render(10, 10);
        let at = |x: usize, y: usize| &px[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
        // The middle of the rectangle is not filled...
        assert_eq!(at(5, 5)[3], 0, "the rect was filled: {:?}", at(5, 5));
        // ...and its edge is stroked green.
        let edge = at(2, 5);
        assert!(edge[1] > 200 && edge[0] < 50 && edge[3] > 0, "{edge:?}");
    }

    /// The buffer is `[r, g, b, a]`: a red square comes out red first.
    #[test]
    fn the_rendered_bytes_are_red_green_blue_alpha() {
        let doc = SvgDocument::parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><rect x="0" y="0" width="4" height="4" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        assert_eq!(&doc.render(4, 4)[..4], &[255, 0, 0, 255]);
    }
}
