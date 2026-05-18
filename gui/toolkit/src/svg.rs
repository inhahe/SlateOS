//! SVG (Subset) renderer for icon rendering and simple vector graphics.
//!
//! Supports enough of the SVG specification for typical application icons and
//! simple illustrations. Does NOT implement the full SVG 2.0 spec.
//!
//! # Supported subset
//!
//! - Basic shapes: rect, circle, ellipse, line, polyline, polygon
//! - Path element with full command set (M, L, H, V, C, S, Q, T, A, Z)
//! - Styling: fill, stroke, stroke-width, opacity, transforms
//! - Container elements: svg (with viewBox), g (with inheritance)
//! - Color parsing: hex, named colors, rgb(), rgba(), none, transparent, currentColor

#![allow(dead_code)]
// Geometry functions inherently need many coordinate parameters.
#![allow(clippy::too_many_arguments)]

use crate::color::Color;
use crate::render::RenderCommand;
use crate::style::CornerRadii;

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
    match hex.len() {
        3 => {
            // #rgb -> #rrggbb
            let r = u8_from_hex_char(hex.as_bytes()[0])?;
            let g = u8_from_hex_char(hex.as_bytes()[1])?;
            let b = u8_from_hex_char(hex.as_bytes()[2])?;
            Ok(Color::rgb(r | (r << 4), g | (g << 4), b | (b << 4)))
        }
        6 => {
            let r = u8_from_hex_pair(hex.as_bytes()[0], hex.as_bytes()[1])?;
            let g = u8_from_hex_pair(hex.as_bytes()[2], hex.as_bytes()[3])?;
            let b = u8_from_hex_pair(hex.as_bytes()[4], hex.as_bytes()[5])?;
            Ok(Color::rgb(r, g, b))
        }
        8 => {
            let r = u8_from_hex_pair(hex.as_bytes()[0], hex.as_bytes()[1])?;
            let g = u8_from_hex_pair(hex.as_bytes()[2], hex.as_bytes()[3])?;
            let b = u8_from_hex_pair(hex.as_bytes()[4], hex.as_bytes()[5])?;
            let a = u8_from_hex_pair(hex.as_bytes()[6], hex.as_bytes()[7])?;
            Ok(Color::rgba(r, g, b, a))
        }
        _ => Err(SvgError::InvalidColor(format!("bad hex length: #{hex}"))),
    }
}

fn u8_from_hex_char(c: u8) -> Result<u8, SvgError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(SvgError::InvalidColor(format!("bad hex char: {}", c as char))),
    }
}

fn u8_from_hex_pair(hi: u8, lo: u8) -> Result<u8, SvgError> {
    Ok(u8_from_hex_char(hi)? << 4 | u8_from_hex_char(lo)?)
}

fn parse_rgb_func(inner: &str) -> Result<Color, SvgError> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return Err(SvgError::InvalidColor(format!("rgb() expects 3 values: {inner}")));
    }
    let r = parse_u8_component(parts[0])?;
    let g = parse_u8_component(parts[1])?;
    let b = parse_u8_component(parts[2])?;
    Ok(Color::rgb(r, g, b))
}

fn parse_rgba_func(inner: &str) -> Result<Color, SvgError> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 4 {
        return Err(SvgError::InvalidColor(format!("rgba() expects 4 values: {inner}")));
    }
    let r = parse_u8_component(parts[0])?;
    let g = parse_u8_component(parts[1])?;
    let b = parse_u8_component(parts[2])?;
    let a_str = parts[3].trim();
    // Alpha can be 0.0-1.0 or 0-255
    let a = if a_str.contains('.') {
        let f: f32 = a_str.parse().map_err(|_| {
            SvgError::InvalidColor(format!("bad alpha: {a_str}"))
        })?;
        (f.clamp(0.0, 1.0) * 255.0) as u8
    } else {
        parse_u8_component(a_str)?
    };
    Ok(Color::rgba(r, g, b, a))
}

fn parse_u8_component(s: &str) -> Result<u8, SvgError> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let f: f32 = pct.trim().parse().map_err(|_| {
            SvgError::InvalidColor(format!("bad percentage: {s}"))
        })?;
        Ok((f.clamp(0.0, 100.0) * 2.55) as u8)
    } else {
        let v: u32 = s.parse().map_err(|_| {
            SvgError::InvalidColor(format!("bad component: {s}"))
        })?;
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
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translate(tx: f32, ty: f32) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx, ty }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self { a: sx, b: 0.0, c: 0.0, d: sy, tx: 0.0, ty: 0.0 }
    }

    pub fn rotate(angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self { a: cos, b: sin, c: -sin, d: cos, tx: 0.0, ty: 0.0 }
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

        // Find function name
        let paren_pos = remaining.find('(').ok_or_else(|| {
            SvgError::InvalidTransform(format!("expected '(' in: {remaining}"))
        })?;
        let func_name = remaining[..paren_pos].trim();
        let close_pos = remaining.find(')').ok_or_else(|| {
            SvgError::InvalidTransform(format!("expected ')' in: {remaining}"))
        })?;
        let args_str = &remaining[paren_pos + 1..close_pos];
        remaining = &remaining[close_pos + 1..];

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
                if args.len() >= 3 {
                    // rotate(angle, cx, cy) — rotate around point
                    let cx = args[1];
                    let cy = args[2];
                    Transform::translate(cx, cy)
                        .then(Transform::rotate(angle))
                        .then(Transform::translate(-cx, -cy))
                } else {
                    Transform::rotate(angle)
                }
            }
            "matrix" => {
                if args.len() < 6 {
                    return Err(SvgError::InvalidTransform(
                        "matrix() requires 6 values".into(),
                    ));
                }
                Transform::matrix(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            "skewX" => {
                let angle = args.first().copied().unwrap_or(0.0) * PI / 180.0;
                Transform { a: 1.0, b: angle.tan(), c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
            }
            "skewY" => {
                let angle = args.first().copied().unwrap_or(0.0) * PI / 180.0;
                Transform { a: 1.0, b: 0.0, c: angle.tan(), d: 1.0, tx: 0.0, ty: 0.0 }
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
            seg.trim().parse::<f32>().map_err(|_| {
                SvgError::InvalidTransform(format!("bad number: {seg}"))
            })
        })
        .collect()
}

// ─── Path Data ───────────────────────────────────────────────────────────────

/// A single command in a parsed SVG path, with absolute coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo { x: f32, y: f32 },
    LineTo { x: f32, y: f32 },
    HorizontalLineTo { x: f32 },
    VerticalLineTo { y: f32 },
    CubicBezier { x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32 },
    SmoothCubic { x2: f32, y2: f32, x: f32, y: f32 },
    QuadraticBezier { x1: f32, y1: f32, x: f32, y: f32 },
    SmoothQuadratic { x: f32, y: f32 },
    Arc { rx: f32, ry: f32, x_rotation: f32, large_arc: bool, sweep: bool, x: f32, y: f32 },
    Close,
}

/// Parse an SVG path `d` attribute into a list of absolute `PathCommand`s.
pub fn parse_path_data(d: &str) -> Result<Vec<PathCommand>, SvgError> {
    let mut commands = Vec::new();
    let mut cursor_x: f32 = 0.0;
    let mut cursor_y: f32 = 0.0;
    let mut start_x: f32 = 0.0;
    let mut start_y: f32 = 0.0;

    let tokens = tokenize_path(d);
    let mut i = 0;

    while i < tokens.len() {
        let token = &tokens[i];
        if token.len() == 1 && token.as_bytes()[0].is_ascii_alphabetic() {
            let cmd_char = token.as_bytes()[0];
            i += 1;
            let is_relative = cmd_char.is_ascii_lowercase();
            let cmd_upper = cmd_char.to_ascii_uppercase();

            match cmd_upper {
                b'M' => {
                    // MoveTo (first pair is moveto, subsequent are lineto)
                    let mut first = true;
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let x_raw = parse_path_number(&tokens[i])?;
                        i += 1;
                        let y_raw = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("M needs y coordinate".into())
                        })?)?;
                        i += 1;

                        let (x, y) = if is_relative {
                            (cursor_x + x_raw, cursor_y + y_raw)
                        } else {
                            (x_raw, y_raw)
                        };

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
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let x_raw = parse_path_number(&tokens[i])?;
                        i += 1;
                        let y_raw = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("L needs y coordinate".into())
                        })?)?;
                        i += 1;

                        let (x, y) = if is_relative {
                            (cursor_x + x_raw, cursor_y + y_raw)
                        } else {
                            (x_raw, y_raw)
                        };
                        commands.push(PathCommand::LineTo { x, y });
                        cursor_x = x;
                        cursor_y = y;
                    }
                }
                b'H' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let x_raw = parse_path_number(&tokens[i])?;
                        i += 1;
                        let x = if is_relative { cursor_x + x_raw } else { x_raw };
                        commands.push(PathCommand::HorizontalLineTo { x });
                        cursor_x = x;
                    }
                }
                b'V' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let y_raw = parse_path_number(&tokens[i])?;
                        i += 1;
                        let y = if is_relative { cursor_y + y_raw } else { y_raw };
                        commands.push(PathCommand::VerticalLineTo { y });
                        cursor_y = y;
                    }
                }
                b'C' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let vals = consume_n_numbers(&tokens, &mut i, 6)?;
                        let (x1, y1, x2, y2, x, y) = if is_relative {
                            (
                                cursor_x + vals[0], cursor_y + vals[1],
                                cursor_x + vals[2], cursor_y + vals[3],
                                cursor_x + vals[4], cursor_y + vals[5],
                            )
                        } else {
                            (vals[0], vals[1], vals[2], vals[3], vals[4], vals[5])
                        };
                        commands.push(PathCommand::CubicBezier { x1, y1, x2, y2, x, y });
                        cursor_x = x;
                        cursor_y = y;
                    }
                }
                b'S' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let vals = consume_n_numbers(&tokens, &mut i, 4)?;
                        let (x2, y2, x, y) = if is_relative {
                            (
                                cursor_x + vals[0], cursor_y + vals[1],
                                cursor_x + vals[2], cursor_y + vals[3],
                            )
                        } else {
                            (vals[0], vals[1], vals[2], vals[3])
                        };
                        commands.push(PathCommand::SmoothCubic { x2, y2, x, y });
                        cursor_x = x;
                        cursor_y = y;
                    }
                }
                b'Q' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let vals = consume_n_numbers(&tokens, &mut i, 4)?;
                        let (x1, y1, x, y) = if is_relative {
                            (
                                cursor_x + vals[0], cursor_y + vals[1],
                                cursor_x + vals[2], cursor_y + vals[3],
                            )
                        } else {
                            (vals[0], vals[1], vals[2], vals[3])
                        };
                        commands.push(PathCommand::QuadraticBezier { x1, y1, x, y });
                        cursor_x = x;
                        cursor_y = y;
                    }
                }
                b'T' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let vals = consume_n_numbers(&tokens, &mut i, 2)?;
                        let (x, y) = if is_relative {
                            (cursor_x + vals[0], cursor_y + vals[1])
                        } else {
                            (vals[0], vals[1])
                        };
                        commands.push(PathCommand::SmoothQuadratic { x, y });
                        cursor_x = x;
                        cursor_y = y;
                    }
                }
                b'A' => {
                    while i < tokens.len() && is_number_token(&tokens[i]) {
                        let rx = parse_path_number(&tokens[i])?.abs();
                        i += 1;
                        let ry = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing ry".into())
                        })?)?
                        .abs();
                        i += 1;
                        let x_rotation = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing x-rotation".into())
                        })?)?;
                        i += 1;
                        let large_arc = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing large-arc flag".into())
                        })?)? != 0.0;
                        i += 1;
                        let sweep = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing sweep flag".into())
                        })?)? != 0.0;
                        i += 1;
                        let x_raw = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing x".into())
                        })?)?;
                        i += 1;
                        let y_raw = parse_path_number(tokens.get(i).ok_or_else(|| {
                            SvgError::InvalidPathData("A: missing y".into())
                        })?)?;
                        i += 1;

                        let (x, y) = if is_relative {
                            (cursor_x + x_raw, cursor_y + y_raw)
                        } else {
                            (x_raw, y_raw)
                        };
                        commands.push(PathCommand::Arc {
                            rx, ry, x_rotation, large_arc, sweep, x, y,
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
                        cmd_char as char
                    )));
                }
            }
        } else {
            // Implicit lineto (bare numbers after initial moveto)
            if is_number_token(token) {
                let x_raw = parse_path_number(token)?;
                i += 1;
                let y_raw = parse_path_number(tokens.get(i).ok_or_else(|| {
                    SvgError::InvalidPathData("implicit lineto needs y".into())
                })?)?;
                i += 1;
                commands.push(PathCommand::LineTo { x: x_raw, y: y_raw });
                cursor_x = x_raw;
                cursor_y = y_raw;
            } else {
                return Err(SvgError::InvalidPathData(format!(
                    "unexpected token: {token}"
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
    let bytes = d.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        match c {
            // Command characters always start a new token
            b'M' | b'm' | b'L' | b'l' | b'H' | b'h' | b'V' | b'v' | b'C' | b'c' | b'S'
            | b's' | b'Q' | b'q' | b'T' | b't' | b'A' | b'a' | b'Z' | b'z' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                tokens.push(String::from(c as char));
                i += 1;
            }
            // Separators
            b',' | b' ' | b'\t' | b'\n' | b'\r' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                i += 1;
            }
            // Minus sign can be separator (start of negative number)
            b'-' => {
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                current.push('-');
                i += 1;
            }
            // Dot can start a new number if we already have a dot
            b'.' => {
                if current.contains('.') {
                    tokens.push(core::mem::take(&mut current));
                }
                current.push('.');
                i += 1;
            }
            // Digits and 'e'/'E' for scientific notation
            b'0'..=b'9' => {
                current.push(c as char);
                i += 1;
            }
            b'e' | b'E' => {
                current.push(c as char);
                i += 1;
            }
            b'+' => {
                // Plus after 'e' is part of scientific notation
                if current.ends_with('e') || current.ends_with('E') {
                    current.push('+');
                } else {
                    if !current.is_empty() {
                        tokens.push(core::mem::take(&mut current));
                    }
                }
                i += 1;
            }
            _ => {
                // Skip unknown characters
                if !current.is_empty() {
                    tokens.push(core::mem::take(&mut current));
                }
                i += 1;
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn is_number_token(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0];
    first == b'-' || first == b'+' || first == b'.' || first.is_ascii_digit()
}

fn parse_path_number(s: &str) -> Result<f32, SvgError> {
    s.parse::<f32>().map_err(|_| {
        SvgError::InvalidPathData(format!("bad number: {s}"))
    })
}

fn consume_n_numbers(tokens: &[String], i: &mut usize, n: usize) -> Result<Vec<f32>, SvgError> {
    let mut vals = Vec::with_capacity(n);
    for _ in 0..n {
        if *i >= tokens.len() || !is_number_token(&tokens[*i]) {
            return Err(SvgError::InvalidPathData(format!(
                "expected {n} numbers, got {}",
                vals.len()
            )));
        }
        vals.push(parse_path_number(&tokens[*i])?);
        *i += 1;
    }
    Ok(vals)
}

// ─── SVG Node Tree ───────────────────────────────────────────────────────────

/// Style properties for an SVG node.
#[derive(Clone, Debug)]
pub struct SvgStyle {
    pub fill: Option<SvgPaint>,
    pub stroke: Option<SvgPaint>,
    pub stroke_width: Option<f32>,
    pub opacity: f32,
    pub fill_opacity: f32,
    pub stroke_opacity: f32,
}

impl Default for SvgStyle {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            stroke_width: None,
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
        if elements.is_empty() {
            return Err(SvgError::MalformedXml("empty document".into()));
        }
        let root = build_node(&elements[0])?;
        Ok(Self { root })
    }

    /// Get the viewBox (min_x, min_y, width, height).
    /// Returns (0, 0, width, height) if no explicit viewBox is set.
    pub fn viewbox(&self) -> (f32, f32, f32, f32) {
        if let SvgNode::Svg { view_box, width, height, .. } = &self.root {
            if let Some(vb) = view_box {
                return *vb;
            }
            return (0.0, 0.0, width.unwrap_or(300.0), height.unwrap_or(150.0));
        }
        (0.0, 0.0, 300.0, 150.0)
    }

    /// Render the SVG to an ARGB pixel buffer at the given dimensions.
    /// Uses 4x supersampling for anti-aliased edges.
    pub fn render(&self, width: u32, height: u32) -> Vec<u8> {
        let mut renderer = SvgRenderer::new(width, height);
        let (vb_x, vb_y, vb_w, vb_h) = self.viewbox();
        let scale_x = width as f32 / vb_w;
        let scale_y = height as f32 / vb_h;
        let base_transform = Transform::scale(scale_x, scale_y)
            .then(Transform::translate(-vb_x, -vb_y));
        renderer.render_node(&self.root, base_transform, &ResolvedStyle::default());
        renderer.buffer
    }

    /// Convert the SVG into a list of `RenderCommand`s positioned at (x, y) with size (w, h).
    /// Rects and lines emit native commands; paths are rasterized to an image.
    pub fn render_commands(&self, x: f32, y: f32, w: f32, h: f32) -> Vec<RenderCommand> {
        let (vb_x, vb_y, vb_w, vb_h) = self.viewbox();
        let scale_x = w / vb_w;
        let scale_y = h / vb_h;

        let mut cmds = Vec::new();
        cmds.push(RenderCommand::PushTranslate { dx: x, dy: y });
        collect_render_commands(
            &self.root,
            Transform::scale(scale_x, scale_y).then(Transform::translate(-vb_x, -vb_y)),
            &ResolvedStyle::default(),
            &mut cmds,
        );
        cmds.push(RenderCommand::PopTranslate);
        cmds
    }
}

// ─── Resolved (Inherited) Style ──────────────────────────────────────────────

/// Style with inheritance resolved — used during rendering traversal.
#[derive(Clone, Debug)]
struct ResolvedStyle {
    fill: SvgPaint,
    stroke: SvgPaint,
    stroke_width: f32,
    opacity: f32,
    fill_opacity: f32,
    stroke_opacity: f32,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        Self {
            fill: SvgPaint::Color(Color::BLACK),
            stroke: SvgPaint::None,
            stroke_width: 1.0,
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
            stroke: style.stroke.unwrap_or(self.stroke),
            stroke_width: style.stroke_width.unwrap_or(self.stroke_width),
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
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn attr_f32(&self, name: &str) -> Option<f32> {
        self.attr(name).and_then(|s| s.parse::<f32>().ok())
    }
}

/// Parse minimal SVG-subset XML.
fn parse_xml(input: &str) -> Result<Vec<XmlElement>, SvgError> {
    let mut pos = 0;
    let bytes = input.as_bytes();

    // Skip BOM, XML declaration, DOCTYPE, comments before root element
    skip_prolog(bytes, &mut pos);

    let mut elements = Vec::new();
    while pos < bytes.len() {
        skip_whitespace(bytes, &mut pos);
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] == b'<' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'/' {
                break; // closing tag — handled by caller
            }
            if pos + 1 < bytes.len() && (bytes[pos + 1] == b'!' || bytes[pos + 1] == b'?') {
                skip_special(bytes, &mut pos);
                continue;
            }
            let elem = parse_element(bytes, &mut pos)?;
            elements.push(elem);
        } else {
            // Skip text content (we don't use text nodes)
            pos += 1;
        }
    }

    Ok(elements)
}

fn skip_prolog(bytes: &[u8], pos: &mut usize) {
    loop {
        skip_whitespace(bytes, pos);
        if *pos >= bytes.len() {
            break;
        }
        if bytes[*pos] == b'<' {
            if *pos + 1 < bytes.len() && (bytes[*pos + 1] == b'?' || bytes[*pos + 1] == b'!') {
                skip_special(bytes, pos);
            } else {
                break;
            }
        } else {
            *pos += 1;
        }
    }
}

fn skip_special(bytes: &[u8], pos: &mut usize) {
    // Skip <!-- comments --> and <?...?> and <!DOCTYPE...>
    if *pos + 3 < bytes.len() && bytes[*pos + 1] == b'!' && bytes[*pos + 2] == b'-' && bytes[*pos + 3] == b'-' {
        // Comment
        *pos += 4;
        while *pos + 2 < bytes.len() {
            if bytes[*pos] == b'-' && bytes[*pos + 1] == b'-' && bytes[*pos + 2] == b'>' {
                *pos += 3;
                return;
            }
            *pos += 1;
        }
        *pos = bytes.len();
    } else {
        // Processing instruction or DOCTYPE — skip to matching '>'
        *pos += 1;
        let mut depth = 1;
        while *pos < bytes.len() && depth > 0 {
            if bytes[*pos] == b'<' {
                depth += 1;
            } else if bytes[*pos] == b'>' {
                depth -= 1;
            }
            *pos += 1;
        }
    }
}

fn skip_whitespace(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn parse_element(bytes: &[u8], pos: &mut usize) -> Result<XmlElement, SvgError> {
    // Expect '<'
    if *pos >= bytes.len() || bytes[*pos] != b'<' {
        return Err(SvgError::MalformedXml("expected '<'".into()));
    }
    *pos += 1;

    // Tag name
    let tag_start = *pos;
    while *pos < bytes.len() && !bytes[*pos].is_ascii_whitespace() && bytes[*pos] != b'>' && bytes[*pos] != b'/' {
        *pos += 1;
    }
    let tag = String::from_utf8_lossy(&bytes[tag_start..*pos]).to_string();

    // Attributes
    let mut attrs = Vec::new();
    loop {
        skip_whitespace(bytes, pos);
        if *pos >= bytes.len() {
            break;
        }
        if bytes[*pos] == b'/' || bytes[*pos] == b'>' {
            break;
        }
        // Attribute name
        let attr_start = *pos;
        while *pos < bytes.len() && bytes[*pos] != b'=' && !bytes[*pos].is_ascii_whitespace() && bytes[*pos] != b'>' && bytes[*pos] != b'/' {
            *pos += 1;
        }
        let attr_name = String::from_utf8_lossy(&bytes[attr_start..*pos]).to_string();
        skip_whitespace(bytes, pos);

        if *pos < bytes.len() && bytes[*pos] == b'=' {
            *pos += 1;
            skip_whitespace(bytes, pos);
            let value = parse_attr_value(bytes, pos)?;
            attrs.push((attr_name, value));
        } else {
            // Boolean attribute (rare in SVG but handle gracefully)
            attrs.push((attr_name, String::new()));
        }
    }

    // Self-closing or opening tag
    let mut children = Vec::new();
    if *pos < bytes.len() && bytes[*pos] == b'/' {
        // Self-closing: <tag ... />
        *pos += 1;
        if *pos < bytes.len() && bytes[*pos] == b'>' {
            *pos += 1;
        }
    } else if *pos < bytes.len() && bytes[*pos] == b'>' {
        *pos += 1;
        // Parse children until closing tag
        loop {
            skip_whitespace(bytes, pos);
            if *pos >= bytes.len() {
                break;
            }
            if *pos + 1 < bytes.len() && bytes[*pos] == b'<' && bytes[*pos + 1] == b'/' {
                // Closing tag
                *pos += 2;
                // Skip tag name and '>'
                while *pos < bytes.len() && bytes[*pos] != b'>' {
                    *pos += 1;
                }
                if *pos < bytes.len() {
                    *pos += 1;
                }
                break;
            }
            if bytes[*pos] == b'<' {
                if *pos + 1 < bytes.len() && (bytes[*pos + 1] == b'!' || bytes[*pos + 1] == b'?') {
                    skip_special(bytes, pos);
                } else {
                    let child = parse_element(bytes, pos)?;
                    children.push(child);
                }
            } else {
                // Skip text content
                *pos += 1;
            }
        }
    }

    Ok(XmlElement { tag, attrs, children })
}

fn parse_attr_value(bytes: &[u8], pos: &mut usize) -> Result<String, SvgError> {
    if *pos >= bytes.len() {
        return Err(SvgError::MalformedXml("expected attribute value".into()));
    }
    let quote = bytes[*pos];
    if quote != b'"' && quote != b'\'' {
        // Unquoted value (non-standard but handle gracefully)
        let start = *pos;
        while *pos < bytes.len() && !bytes[*pos].is_ascii_whitespace() && bytes[*pos] != b'>' && bytes[*pos] != b'/' {
            *pos += 1;
        }
        return Ok(String::from_utf8_lossy(&bytes[start..*pos]).to_string());
    }
    *pos += 1; // skip opening quote
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos] != quote {
        *pos += 1;
    }
    let value = String::from_utf8_lossy(&bytes[start..*pos]).to_string();
    if *pos < bytes.len() {
        *pos += 1; // skip closing quote
    }
    Ok(value)
}

// ─── Node Builder ────────────────────────────────────────────────────────────

fn build_node(elem: &XmlElement) -> Result<SvgNode, SvgError> {
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

    let children: Result<Vec<_>, _> = elem.children.iter().map(build_node).collect();
    Ok(SvgNode::Svg {
        width,
        height,
        view_box,
        children: children?,
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
    Ok(SvgNode::Rect {
        x: elem.attr_f32("x").unwrap_or(0.0),
        y: elem.attr_f32("y").unwrap_or(0.0),
        width: elem.attr_f32("width").unwrap_or(0.0),
        height: elem.attr_f32("height").unwrap_or(0.0),
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

fn parse_style_attrs(elem: &XmlElement) -> Result<SvgStyle, SvgError> {
    let fill = elem.attr("fill").map(parse_color).transpose()?;
    let stroke = elem.attr("stroke").map(parse_color).transpose()?;
    let stroke_width = elem.attr_f32("stroke-width");
    let opacity = elem.attr_f32("opacity").unwrap_or(1.0);
    let fill_opacity = elem.attr_f32("fill-opacity").unwrap_or(1.0);
    let stroke_opacity = elem.attr_f32("stroke-opacity").unwrap_or(1.0);

    Ok(SvgStyle {
        fill,
        stroke,
        stroke_width,
        opacity,
        fill_opacity,
        stroke_opacity,
    })
}

fn parse_viewbox(s: &str) -> Result<(f32, f32, f32, f32), SvgError> {
    let parts: Vec<f32> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SvgError::MalformedXml(format!("bad viewBox: {s}")))?;
    if parts.len() != 4 {
        return Err(SvgError::MalformedXml(format!("viewBox needs 4 values: {s}")));
    }
    Ok((parts[0], parts[1], parts[2], parts[3]))
}

fn parse_points(s: &str) -> Result<Vec<(f32, f32)>, SvgError> {
    let numbers: Vec<f32> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| seg.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SvgError::InvalidPathData(format!("bad points: {s}")))?;
    if !numbers.len().is_multiple_of(2) {
        return Err(SvgError::InvalidPathData("points needs even number of values".into()));
    }
    Ok(numbers.chunks(2).map(|c| (c[0], c[1])).collect())
}

// ─── Bézier Flattening ───────────────────────────────────────────────────────

/// Default flatness threshold in pixels.
const DEFAULT_FLATNESS: f32 = 0.25;

/// Flatten a cubic Bézier curve to line segments using adaptive subdivision.
fn flatten_cubic(
    x0: f32, y0: f32,
    x1: f32, y1: f32,
    x2: f32, y2: f32,
    x3: f32, y3: f32,
    flatness: f32,
    output: &mut Vec<(f32, f32)>,
) {
    flatten_cubic_recursive(x0, y0, x1, y1, x2, y2, x3, y3, flatness * flatness, 0, output);
}

fn flatten_cubic_recursive(
    x0: f32, y0: f32,
    x1: f32, y1: f32,
    x2: f32, y2: f32,
    x3: f32, y3: f32,
    flatness_sq: f32,
    depth: u32,
    output: &mut Vec<(f32, f32)>,
) {
    // Maximum recursion depth to avoid stack overflow on degenerate curves
    if depth > 16 {
        output.push((x3, y3));
        return;
    }

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

    flatten_cubic_recursive(x0, y0, mx01, my01, mx012, my012, mx0123, my0123, flatness_sq, depth + 1, output);
    flatten_cubic_recursive(mx0123, my0123, mx123, my123, mx23, my23, x3, y3, flatness_sq, depth + 1, output);
}

/// Flatten a quadratic Bézier curve to line segments.
fn flatten_quadratic(
    x0: f32, y0: f32,
    x1: f32, y1: f32,
    x2: f32, y2: f32,
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
    cursor_x: f32, cursor_y: f32,
    rx: f32, ry: f32,
    x_rotation: f32,
    large_arc: bool,
    sweep: bool,
    target_x: f32, target_y: f32,
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
    let sq = if denom < 1e-10 { 0.0 } else { (num / denom).sqrt() };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = sign * sq * (rx * y1p / ry);
    let cyp = sign * sq * -(ry * x1p / rx);

    // Step 3: compute (cx, cy) from (cx', cy')
    let cx = cos_phi * cxp - sin_phi * cyp + (cursor_x + target_x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (cursor_y + target_y) / 2.0;

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

    // Approximate with line segments
    let n_segs = ((dtheta.abs() / (PI / 4.0)).ceil() as u32).max(1);
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
    if ux * vy - uy * vx < 0.0 { -angle } else { angle }
}

/// Convert path commands into a series of polygon outlines (lists of points).
fn path_to_polygons(commands: &[PathCommand], transform: Transform) -> Vec<Vec<(f32, f32)>> {
    let mut polygons: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    let mut cursor_x: f32 = 0.0;
    let mut cursor_y: f32 = 0.0;
    let mut start_x: f32 = 0.0;
    let mut start_y: f32 = 0.0;
    // For smooth curves, we track the last control point
    let mut last_cubic_cp: Option<(f32, f32)> = None;
    let mut last_quad_cp: Option<(f32, f32)> = None;

    for cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if !current.is_empty() {
                    polygons.push(core::mem::take(&mut current));
                }
                let (tx, ty) = transform.apply(*x, *y);
                current.push((tx, ty));
                cursor_x = *x;
                cursor_y = *y;
                start_x = *x;
                start_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::LineTo { x, y } => {
                let (tx, ty) = transform.apply(*x, *y);
                current.push((tx, ty));
                cursor_x = *x;
                cursor_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::HorizontalLineTo { x } => {
                let (tx, ty) = transform.apply(*x, cursor_y);
                current.push((tx, ty));
                cursor_x = *x;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::VerticalLineTo { y } => {
                let (tx, ty) = transform.apply(cursor_x, *y);
                current.push((tx, ty));
                cursor_y = *y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
            PathCommand::CubicBezier { x1, y1, x2, y2, x, y } => {
                let (tx0, ty0) = transform.apply(cursor_x, cursor_y);
                let (tx1, ty1) = transform.apply(*x1, *y1);
                let (tx2, ty2) = transform.apply(*x2, *y2);
                let (tx3, ty3) = transform.apply(*x, *y);
                flatten_cubic(tx0, ty0, tx1, ty1, tx2, ty2, tx3, ty3, DEFAULT_FLATNESS, &mut current);
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
                flatten_cubic(tx0, ty0, tx1, ty1, tx2, ty2, tx3, ty3, DEFAULT_FLATNESS, &mut current);
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
                last_quad_cp = Some((rx1, ry1));
                last_cubic_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::Arc { rx, ry, x_rotation, large_arc, sweep, x, y } => {
                // We flatten in untransformed space then transform points
                let mut arc_pts = Vec::new();
                flatten_arc(cursor_x, cursor_y, *rx, *ry, *x_rotation, *large_arc, *sweep, *x, *y, &mut arc_pts);
                for (px, py) in &arc_pts {
                    let (tx, ty) = transform.apply(*px, *py);
                    current.push((tx, ty));
                }
                last_cubic_cp = None;
                last_quad_cp = None;
                cursor_x = *x;
                cursor_y = *y;
            }
            PathCommand::Close => {
                let (tx, ty) = transform.apply(start_x, start_y);
                current.push((tx, ty));
                if !current.is_empty() {
                    polygons.push(core::mem::take(&mut current));
                }
                cursor_x = start_x;
                cursor_y = start_y;
                last_cubic_cp = None;
                last_quad_cp = None;
            }
        }
    }

    if !current.is_empty() {
        polygons.push(current);
    }

    polygons
}

// ─── Scanline Rasterizer ─────────────────────────────────────────────────────

/// Software rasterizer that renders SVG to a pixel buffer.
struct SvgRenderer {
    width: u32,
    height: u32,
    /// ARGB buffer (4 bytes per pixel: [B, G, R, A] in little-endian, or as u32 ARGB)
    buffer: Vec<u8>,
    /// 2x supersampling grid for anti-aliasing
    ss_factor: u32,
}

impl SvgRenderer {
    fn new(width: u32, height: u32) -> Self {
        let size = (width as usize) * (height as usize) * 4;
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
            SvgNode::Group { transform: local_xf, style, children } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                for child in children {
                    self.render_node(child, combined, &resolved);
                }
            }
            SvgNode::Rect { x, y, width, height, rx, ry, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                self.render_rect(*x, *y, *width, *height, *rx, *ry, combined, &resolved);
            }
            SvgNode::Circle { cx, cy, r, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                self.render_ellipse(*cx, *cy, *r, *r, combined, &resolved);
            }
            SvgNode::Ellipse { cx, cy, rx, ry, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                self.render_ellipse(*cx, *cy, *rx, *ry, combined, &resolved);
            }
            SvgNode::Line { x1, y1, x2, y2, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                if let Some(color) = resolved.effective_stroke_color() {
                    let (tx1, ty1) = combined.apply(*x1, *y1);
                    let (tx2, ty2) = combined.apply(*x2, *y2);
                    self.draw_line(tx1, ty1, tx2, ty2, resolved.stroke_width, color);
                }
            }
            SvgNode::Polyline { points, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let transformed: Vec<(f32, f32)> = points.iter().map(|(px, py)| combined.apply(*px, *py)).collect();
                if let Some(color) = resolved.effective_stroke_color() {
                    for w in transformed.windows(2) {
                        self.draw_line(w[0].0, w[0].1, w[1].0, w[1].1, resolved.stroke_width, color);
                    }
                }
            }
            SvgNode::Polygon { points, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let transformed: Vec<(f32, f32)> = points.iter().map(|(px, py)| combined.apply(*px, *py)).collect();
                if let Some(fill_color) = resolved.effective_fill_color() {
                    self.fill_polygon(&transformed, fill_color);
                }
                if let Some(stroke_color) = resolved.effective_stroke_color() {
                    for w in transformed.windows(2) {
                        self.draw_line(w[0].0, w[0].1, w[1].0, w[1].1, resolved.stroke_width, stroke_color);
                    }
                    if transformed.len() >= 2 {
                        let last = transformed.len() - 1;
                        self.draw_line(
                            transformed[last].0, transformed[last].1,
                            transformed[0].0, transformed[0].1,
                            resolved.stroke_width, stroke_color,
                        );
                    }
                }
            }
            SvgNode::Path { commands, transform: local_xf, style } => {
                let combined = transform.then(*local_xf);
                let resolved = parent_style.with_overrides(style);
                let polygons = path_to_polygons(commands, combined);
                if let Some(fill_color) = resolved.effective_fill_color() {
                    for poly in &polygons {
                        self.fill_polygon(poly, fill_color);
                    }
                }
                if let Some(stroke_color) = resolved.effective_stroke_color() {
                    for poly in &polygons {
                        for w in poly.windows(2) {
                            self.draw_line(w[0].0, w[0].1, w[1].0, w[1].1, resolved.stroke_width, stroke_color);
                        }
                    }
                }
            }
        }
    }

    fn render_rect(
        &mut self,
        x: f32, y: f32, w: f32, h: f32,
        rx: f32, ry: f32,
        transform: Transform,
        style: &ResolvedStyle,
    ) {
        if rx <= 0.0 && ry <= 0.0 {
            // Simple rectangle — generate 4-point polygon
            let corners = [
                transform.apply(x, y),
                transform.apply(x + w, y),
                transform.apply(x + w, y + h),
                transform.apply(x, y + h),
            ];
            let poly: Vec<(f32, f32)> = corners.to_vec();
            if let Some(fill_color) = style.effective_fill_color() {
                self.fill_polygon(&poly, fill_color);
            }
            if let Some(stroke_color) = style.effective_stroke_color() {
                for i in 0..4 {
                    let j = (i + 1) % 4;
                    self.draw_line(poly[i].0, poly[i].1, poly[j].0, poly[j].1, style.stroke_width, stroke_color);
                }
            }
        } else {
            // Rounded rectangle — approximate corners with arcs
            let rx = rx.min(w / 2.0);
            let ry = ry.min(h / 2.0);
            let mut points = Vec::new();
            let segments_per_corner = 8;

            // Top-right corner
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = -PI / 2.0 + t * (PI / 2.0);
                let px = x + w - rx + rx * angle.cos();
                let py = y + ry + ry * angle.sin();
                points.push(transform.apply(px, py));
            }
            // Bottom-right corner
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = t * (PI / 2.0);
                let px = x + w - rx + rx * angle.cos();
                let py = y + h - ry + ry * angle.sin();
                points.push(transform.apply(px, py));
            }
            // Bottom-left corner
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = PI / 2.0 + t * (PI / 2.0);
                let px = x + rx + rx * angle.cos();
                let py = y + h - ry + ry * angle.sin();
                points.push(transform.apply(px, py));
            }
            // Top-left corner
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = PI + t * (PI / 2.0);
                let px = x + rx + rx * angle.cos();
                let py = y + ry + ry * angle.sin();
                points.push(transform.apply(px, py));
            }

            if let Some(fill_color) = style.effective_fill_color() {
                self.fill_polygon(&points, fill_color);
            }
            if let Some(stroke_color) = style.effective_stroke_color() {
                for w in points.windows(2) {
                    self.draw_line(w[0].0, w[0].1, w[1].0, w[1].1, style.stroke_width, stroke_color);
                }
                if points.len() >= 2 {
                    let last = points.len() - 1;
                    self.draw_line(points[last].0, points[last].1, points[0].0, points[0].1, style.stroke_width, stroke_color);
                }
            }
        }
    }

    fn render_ellipse(
        &mut self,
        cx: f32, cy: f32, rx: f32, ry: f32,
        transform: Transform,
        style: &ResolvedStyle,
    ) {
        // Approximate ellipse as polygon
        let n_segments = 32u32;
        let points: Vec<(f32, f32)> = (0..n_segments)
            .map(|i| {
                let angle = 2.0 * PI * (i as f32 / n_segments as f32);
                let px = cx + rx * angle.cos();
                let py = cy + ry * angle.sin();
                transform.apply(px, py)
            })
            .collect();

        if let Some(fill_color) = style.effective_fill_color() {
            self.fill_polygon(&points, fill_color);
        }
        if let Some(stroke_color) = style.effective_stroke_color() {
            for i in 0..points.len() {
                let j = (i + 1) % points.len();
                self.draw_line(points[i].0, points[i].1, points[j].0, points[j].1, style.stroke_width, stroke_color);
            }
        }
    }

    /// Fill a polygon using the even-odd scanline rule with 4x vertical supersampling.
    fn fill_polygon(&mut self, points: &[(f32, f32)], color: Color) {
        if points.len() < 3 {
            return;
        }

        // Find bounding box
        let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
        let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);

        let y_start = (min_y.floor() as i32).max(0);
        let y_end = (max_y.ceil() as i32).min(self.height as i32);
        let x_start = (min_x.floor() as i32).max(0);
        let x_end = (max_x.ceil() as i32).min(self.width as i32);

        if y_start >= y_end || x_start >= x_end {
            return;
        }

        let ss = self.ss_factor;
        let ss_f = ss as f32;

        // For each pixel row, subsample vertically
        for py in y_start..y_end {
            // Accumulate coverage per pixel column
            let x_range = (x_end - x_start) as usize;
            let mut coverage = vec![0u32; x_range];

            for sub_y in 0..ss {
                let scan_y = py as f32 + (sub_y as f32 + 0.5) / ss_f;

                // Find all x-intersections for this scanline
                let mut intersections = Vec::new();
                let n = points.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (_, y0) = points[i];
                    let (_, y1) = points[j];

                    if (y0 <= scan_y && y1 > scan_y) || (y1 <= scan_y && y0 > scan_y) {
                        let (x0, _) = points[i];
                        let (x1, _) = points[j];
                        let t = (scan_y - y0) / (y1 - y0);
                        let ix = x0 + t * (x1 - x0);
                        intersections.push(ix);
                    }
                }

                intersections.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

                // Even-odd rule: fill between pairs of intersections
                for pair in intersections.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    let left = pair[0].max(x_start as f32);
                    let right = pair[1].min(x_end as f32);
                    if left >= right {
                        continue;
                    }

                    let col_start = (left.floor() as i32 - x_start).max(0) as usize;
                    let col_end = (right.ceil() as i32 - x_start).min(x_range as i32) as usize;

                    for (col_idx, cov_slot) in coverage[col_start..col_end].iter_mut().enumerate() {
                        let px_left = ((col_idx + col_start) as i32 + x_start) as f32;
                        let px_right = px_left + 1.0;
                        // Calculate horizontal coverage for this sub-pixel row
                        let covered_left = left.max(px_left);
                        let covered_right = right.min(px_right);
                        if covered_right > covered_left {
                            // Quantize sub-pixel coverage to integer (out of ss*ss)
                            let frac = ((covered_right - covered_left) * ss_f) as u32;
                            *cov_slot += frac;
                        }
                    }
                }
            }

            // Blend pixels based on accumulated coverage
            let total_ss = ss * ss;
            for (col_idx, &cov) in coverage.iter().enumerate() {
                if cov == 0 {
                    continue;
                }
                let px = (col_idx as i32 + x_start) as u32;
                let alpha = ((cov.min(total_ss) as f32 / total_ss as f32) * color.a as f32) as u8;
                let c = Color::rgba(color.r, color.g, color.b, alpha);
                self.blend_pixel(px, py as u32, c);
            }
        }
    }

    /// Draw a line with the given width using simple rectangle expansion.
    fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Color) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            return;
        }

        // Normal to the line
        let nx = -dy / len * (width / 2.0);
        let ny = dx / len * (width / 2.0);

        // Line as a 4-point polygon
        let poly = [
            (x1 + nx, y1 + ny),
            (x2 + nx, y2 + ny),
            (x2 - nx, y2 - ny),
            (x1 - nx, y1 - ny),
        ];
        self.fill_polygon(&poly, color);
    }

    /// Blend a single pixel (alpha compositing).
    fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        if offset + 3 >= self.buffer.len() {
            return;
        }

        // Buffer format: [R, G, B, A]
        let dst = Color::rgba(
            self.buffer[offset],
            self.buffer[offset + 1],
            self.buffer[offset + 2],
            self.buffer[offset + 3],
        );
        let result = color.over(dst);
        self.buffer[offset] = result.r;
        self.buffer[offset + 1] = result.g;
        self.buffer[offset + 2] = result.b;
        self.buffer[offset + 3] = result.a;
    }
}

// ─── RenderCommand Generation ────────────────────────────────────────────────

/// Collect RenderCommands from an SVG node tree (for compositor integration).
fn collect_render_commands(
    node: &SvgNode,
    transform: Transform,
    parent_style: &ResolvedStyle,
    cmds: &mut Vec<RenderCommand>,
) {
    match node {
        SvgNode::Svg { children, .. } => {
            for child in children {
                collect_render_commands(child, transform, parent_style, cmds);
            }
        }
        SvgNode::Group { transform: local_xf, style, children } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            for child in children {
                collect_render_commands(child, combined, &resolved, cmds);
            }
        }
        SvgNode::Rect { x, y, width, height, rx, ry, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            // For axis-aligned rects without rotation, emit native FillRect
            let (tx, ty) = combined.apply(*x, *y);
            let (tx2, ty2) = combined.apply(*x + *width, *y + *height);
            let rw = tx2 - tx;
            let rh = ty2 - ty;

            let corner_radii = if *rx > 0.0 || *ry > 0.0 {
                let r = rx.max(*ry);
                // Scale radius by transform
                let scale = ((combined.a * combined.a + combined.c * combined.c).sqrt()
                    + (combined.b * combined.b + combined.d * combined.d).sqrt())
                    / 2.0;
                let sr = r * scale;
                CornerRadii {
                    top_left: sr,
                    top_right: sr,
                    bottom_right: sr,
                    bottom_left: sr,
                }
            } else {
                CornerRadii::ZERO
            };

            if let Some(fill_color) = resolved.effective_fill_color() {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: ty,
                    width: rw,
                    height: rh,
                    color: fill_color,
                    corner_radii,
                });
            }
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                cmds.push(RenderCommand::StrokeRect {
                    x: tx,
                    y: ty,
                    width: rw,
                    height: rh,
                    color: stroke_color,
                    line_width: resolved.stroke_width,
                    corner_radii,
                });
            }
        }
        SvgNode::Line { x1, y1, x2, y2, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                let (tx1, ty1) = combined.apply(*x1, *y1);
                let (tx2, ty2) = combined.apply(*x2, *y2);
                cmds.push(RenderCommand::Line {
                    x1: tx1,
                    y1: ty1,
                    x2: tx2,
                    y2: ty2,
                    color: stroke_color,
                    width: resolved.stroke_width,
                });
            }
        }
        SvgNode::Polyline { points, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                for w in points.windows(2) {
                    let (tx1, ty1) = combined.apply(w[0].0, w[0].1);
                    let (tx2, ty2) = combined.apply(w[1].0, w[1].1);
                    cmds.push(RenderCommand::Line {
                        x1: tx1,
                        y1: ty1,
                        x2: tx2,
                        y2: ty2,
                        color: stroke_color,
                        width: resolved.stroke_width,
                    });
                }
            }
        }
        // For complex shapes (circles, ellipses, polygons, paths), we would
        // ideally rasterize to a temporary buffer and emit as an Image command.
        // For now, emit FillRects for polygons and approximate circles/ellipses.
        SvgNode::Circle { cx, cy, r, transform: local_xf, style }
        | SvgNode::Ellipse { cx, cy, rx: r, ry: _, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            let ry_val = match node {
                SvgNode::Ellipse { ry, .. } => *ry,
                _ => *r,
            };
            // Approximate as a FillRect with full corner radii
            let (tx, ty) = combined.apply(*cx - *r, *cy - ry_val);
            let (tx2, ty2) = combined.apply(*cx + *r, *cy + ry_val);
            let rw = tx2 - tx;
            let rh = ty2 - ty;
            let radii = CornerRadii {
                top_left: rw.min(rh) / 2.0,
                top_right: rw.min(rh) / 2.0,
                bottom_right: rw.min(rh) / 2.0,
                bottom_left: rw.min(rh) / 2.0,
            };
            if let Some(fill_color) = resolved.effective_fill_color() {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: ty,
                    width: rw,
                    height: rh,
                    color: fill_color,
                    corner_radii: radii,
                });
            }
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                cmds.push(RenderCommand::StrokeRect {
                    x: tx,
                    y: ty,
                    width: rw,
                    height: rh,
                    color: stroke_color,
                    line_width: resolved.stroke_width,
                    corner_radii: radii,
                });
            }
        }
        SvgNode::Polygon { points, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            // Emit as line segments for stroke
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                for w in points.windows(2) {
                    let (tx1, ty1) = combined.apply(w[0].0, w[0].1);
                    let (tx2, ty2) = combined.apply(w[1].0, w[1].1);
                    cmds.push(RenderCommand::Line {
                        x1: tx1, y1: ty1, x2: tx2, y2: ty2,
                        color: stroke_color, width: resolved.stroke_width,
                    });
                }
                if points.len() >= 2 {
                    let (tx1, ty1) = combined.apply(points.last().map(|p| p.0).unwrap_or(0.0), points.last().map(|p| p.1).unwrap_or(0.0));
                    let (tx2, ty2) = combined.apply(points[0].0, points[0].1);
                    cmds.push(RenderCommand::Line {
                        x1: tx1, y1: ty1, x2: tx2, y2: ty2,
                        color: stroke_color, width: resolved.stroke_width,
                    });
                }
            }
            // Fill approximation: bounding rect
            if let Some(fill_color) = resolved.effective_fill_color() {
                let transformed: Vec<(f32, f32)> = points.iter().map(|(px, py)| combined.apply(*px, *py)).collect();
                if !transformed.is_empty() {
                    let min_x = transformed.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                    let max_x = transformed.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                    let min_y = transformed.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                    let max_y = transformed.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                    cmds.push(RenderCommand::FillRect {
                        x: min_x, y: min_y,
                        width: max_x - min_x, height: max_y - min_y,
                        color: fill_color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
        SvgNode::Path { commands, transform: local_xf, style } => {
            let combined = transform.then(*local_xf);
            let resolved = parent_style.with_overrides(style);
            let polygons = path_to_polygons(commands, combined);
            // Emit strokes as Line commands
            if let Some(stroke_color) = resolved.effective_stroke_color() {
                for poly in &polygons {
                    for w in poly.windows(2) {
                        cmds.push(RenderCommand::Line {
                            x1: w[0].0, y1: w[0].1,
                            x2: w[1].0, y2: w[1].1,
                            color: stroke_color,
                            width: resolved.stroke_width,
                        });
                    }
                }
            }
            // Fill approximation: bounding rect per polygon
            if let Some(fill_color) = resolved.effective_fill_color() {
                for poly in &polygons {
                    if poly.len() < 3 {
                        continue;
                    }
                    let min_x = poly.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                    let max_x = poly.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                    let min_y = poly.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                    let max_y = poly.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                    cmds.push(RenderCommand::FillRect {
                        x: min_x, y: min_y,
                        width: max_x - min_x, height: max_y - min_y,
                        color: fill_color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- XML parsing tests ---

    #[test]
    fn test_parse_xml_basic() {
        let svg = r#"<svg width="100" height="100"><rect x="10" y="20" width="30" height="40"/></svg>"#;
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
            PathCommand::CubicBezier { x1: 10.0, y1: 20.0, x2: 30.0, y2: 40.0, x: 50.0, y: 60.0 }
        );
    }

    #[test]
    fn test_path_quadratic() {
        let cmds = parse_path_data("M 0 0 Q 10 20 30 40").unwrap();
        assert_eq!(
            cmds[1],
            PathCommand::QuadraticBezier { x1: 10.0, y1: 20.0, x: 30.0, y: 40.0 }
        );
    }

    #[test]
    fn test_path_arc() {
        let cmds = parse_path_data("M 0 0 A 25 25 0 0 1 50 50").unwrap();
        assert_eq!(
            cmds[1],
            PathCommand::Arc {
                rx: 25.0, ry: 25.0, x_rotation: 0.0,
                large_arc: false, sweep: true, x: 50.0, y: 50.0,
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
        assert_eq!(cmds[2], PathCommand::SmoothCubic { x2: 70.0, y2: 80.0, x: 90.0, y: 100.0 });
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
        assert_eq!(parse_color("red").unwrap(), SvgPaint::Color(Color::rgb(255, 0, 0)));
        assert_eq!(parse_color("blue").unwrap(), SvgPaint::Color(Color::rgb(0, 0, 255)));
        assert_eq!(parse_color("green").unwrap(), SvgPaint::Color(Color::rgb(0, 128, 0)));
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
        assert_eq!(parse_color("transparent").unwrap(), SvgPaint::Color(Color::TRANSPARENT));
    }

    #[test]
    fn test_color_current_color() {
        assert_eq!(parse_color("currentColor").unwrap(), SvgPaint::CurrentColor);
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
        flatten_cubic(0.0, 0.0, 10.0, 10.0, 20.0, 20.0, 30.0, 30.0, 0.25, &mut output);
        // Should be very few points since it's flat
        assert!(!output.is_empty());
        assert!(output.len() <= 4); // Should be just 1 point (the end)
    }

    #[test]
    fn test_flatten_curve_produces_points() {
        // A real curve should produce more points
        let mut output = Vec::new();
        flatten_cubic(0.0, 0.0, 0.0, 100.0, 100.0, 100.0, 100.0, 0.0, 0.25, &mut output);
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
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100" width="200" height="200"></svg>"#,
        ).unwrap();
        assert_eq!(doc.viewbox(), (0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn test_viewbox_default() {
        let doc = SvgDocument::parse(
            r#"<svg width="300" height="150"></svg>"#,
        ).unwrap();
        assert_eq!(doc.viewbox(), (0.0, 0.0, 300.0, 150.0));
    }

    #[test]
    fn test_viewbox_offset() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="10 20 80 60"></svg>"#,
        ).unwrap();
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
                    SvgNode::Rect { x, y, width, height, style, .. } => {
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
        ).unwrap();
        match &doc.root {
            SvgNode::Svg { children, .. } => {
                match &children[0] {
                    SvgNode::Group { transform, style, children } => {
                        assert!((transform.tx - 5.0).abs() < 1e-5);
                        assert!((transform.ty - 5.0).abs() < 1e-5);
                        assert_eq!(style.fill, Some(SvgPaint::Color(Color::rgb(0, 0, 255))));
                        assert_eq!(children.len(), 1);
                    }
                    _ => panic!("expected Group node"),
                }
            }
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
        assert_eq!(buf[center], 255);     // R
        assert_eq!(buf[center + 1], 0);   // G
        assert_eq!(buf[center + 2], 0);   // B
        assert_eq!(buf[center + 3], 255); // A
    }

    #[test]
    fn test_render_circle_center_filled() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="40" fill="blue"/></svg>"#,
        ).unwrap();
        let buf = doc.render(100, 100);
        // Center should be filled with blue
        let center = (50 * 100 + 50) * 4;
        assert_eq!(buf[center], 0);       // R
        assert_eq!(buf[center + 1], 0);   // G
        assert_eq!(buf[center + 2], 255); // B
        assert!(buf[center + 3] > 200);   // A (should be full or near-full)
    }

    #[test]
    fn test_render_commands_rect() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100"><rect x="10" y="20" width="30" height="40" fill="green"/></svg>"#,
        ).unwrap();
        let cmds = doc.render_commands(0.0, 0.0, 100.0, 100.0);
        // Should have PushTranslate, FillRect, PopTranslate
        assert!(cmds.len() >= 3);
        // First and last should be translate/untranslate
        assert!(matches!(cmds[0], RenderCommand::PushTranslate { .. }));
        assert!(matches!(cmds.last().unwrap(), RenderCommand::PopTranslate));
    }

    #[test]
    fn test_render_commands_line() {
        let doc = SvgDocument::parse(
            r#"<svg viewBox="0 0 100 100"><line x1="0" y1="0" x2="100" y2="100" stroke="black" stroke-width="2"/></svg>"#,
        ).unwrap();
        let cmds = doc.render_commands(0.0, 0.0, 200.0, 200.0);
        let has_line = cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. }));
        assert!(has_line);
    }
}
