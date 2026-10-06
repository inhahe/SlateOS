//! The values a property is given: lengths in any of CSS's units and their
//! sums (`calc()`), numbers, and colours.
//!
//! A length is kept as a sum of amounts in each unit ([`Length`]) until it
//! is drawn, because what `1em` or `50%` is depends on where the widget is
//! -- its font, its parent's width, the window -- and `calc(100% - 2em)`
//! has to stay a sum of the two until both are known.

use super::token::{Spanned, Token};
use crate::color::Color;

/// A length, or a sum of lengths in different units: `calc(100% - 2em)` is
/// `percent: 100.0, em: -2.0`.
///
/// `cm` is kept as ten `mm`, and `in`, `pt` and `pc` are not read: the
/// toolkit is not for print.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Length {
    /// Pixels.
    pub px: f32,
    /// The font size of the widget it is given to (its parent's, for
    /// `font-size` itself).
    pub em: f32,
    /// The base font size: the user's text size.
    pub rem: f32,
    /// The width of a `0` in the widget's font.
    pub ch: f32,
    /// Hundredths of the window's width.
    pub vw: f32,
    /// Hundredths of the window's height.
    pub vh: f32,
    /// Millimetres on the display, where its size is known.
    pub mm: f32,
    /// Hundredths of whatever the property measures against.
    pub percent: f32,
}

/// What a [`Length`]'s units are worth where it is used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    /// What `1em` is, in pixels.
    pub em: f32,
    /// What `1rem` is.
    pub rem: f32,
    /// What `1ch` is.
    pub ch: f32,
    /// The window's width and height, for `vw` and `vh`.
    pub viewport: (f32, f32),
    /// Pixels to a millimetre: CSS's 96 to the inch unless the display's
    /// size is known.
    pub px_per_mm: f32,
}

impl Units {
    /// CSS's reference pixel: 96 to the inch.
    pub const REFERENCE_PX_PER_MM: f32 = 96.0 / 25.4;
}

impl Length {
    /// `px` pixels.
    #[must_use]
    pub const fn px(px: f32) -> Self {
        Self {
            px,
            em: 0.0,
            rem: 0.0,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            mm: 0.0,
            percent: 0.0,
        }
    }

    /// Whether any of it is a percentage, which needs something to be a
    /// percentage of.
    #[must_use]
    pub fn has_percent(&self) -> bool {
        self.percent != 0.0
    }

    /// In pixels, with `units` and a percentage of `percent_of`.
    #[must_use]
    pub fn resolve(&self, units: &Units, percent_of: f32) -> f32 {
        let total = self.px
            + self.em * units.em
            + self.rem * units.rem
            + self.ch * units.ch
            + self.vw * units.viewport.0 / 100.0
            + self.vh * units.viewport.1 / 100.0
            + self.mm * units.px_per_mm
            + self.percent * percent_of / 100.0;
        if total.is_finite() { total } else { 0.0 }
    }

    fn scaled(self, k: f32) -> Self {
        Self {
            px: self.px * k,
            em: self.em * k,
            rem: self.rem * k,
            ch: self.ch * k,
            vw: self.vw * k,
            vh: self.vh * k,
            mm: self.mm * k,
            percent: self.percent * k,
        }
    }

    fn plus(self, other: Self) -> Self {
        Self {
            px: self.px + other.px,
            em: self.em + other.em,
            rem: self.rem + other.rem,
            ch: self.ch + other.ch,
            vw: self.vw + other.vw,
            vh: self.vh + other.vh,
            mm: self.mm + other.mm,
            percent: self.percent + other.percent,
        }
    }

    /// Whether it is negative whatever its units turn out to be worth: every
    /// amount in it nought or less, and one less. A sum of signs, as
    /// `calc(100% - 2em)` is, is not: where it comes to less than nought it
    /// is held to nought, as CSS holds it.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        let amounts = [
            self.px,
            self.em,
            self.rem,
            self.ch,
            self.vw,
            self.vh,
            self.mm,
            self.percent,
        ];
        amounts.iter().all(|v| *v <= 0.0) && amounts.iter().any(|v| *v < 0.0)
    }
}

/// The length `value` of the unit `unit`, or `None` for a unit this does
/// not read.
fn length_in(value: f32, unit: &str) -> Option<Length> {
    let mut l = Length::default();
    match unit.to_ascii_lowercase().as_str() {
        "px" => l.px = value,
        "em" => l.em = value,
        "rem" => l.rem = value,
        "ch" => l.ch = value,
        "vw" => l.vw = value,
        "vh" => l.vh = value,
        "mm" => l.mm = value,
        "cm" => l.mm = value * 10.0,
        _ => return None,
    }
    Some(l)
}

// ---------------------------------------------------------------------------
// Reading tokens
// ---------------------------------------------------------------------------

/// The tokens of one value, and how far through them a reader is.
#[derive(Clone, Copy, Debug)]
pub struct Cursor<'a> {
    tokens: &'a [Spanned],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// A reader at the start of `tokens`.
    #[must_use]
    pub const fn new(tokens: &'a [Spanned]) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Past any whitespace.
    pub fn skip_ws(&mut self) {
        while self.peek_raw() == Some(&Token::Whitespace) {
            self.pos = self.pos.saturating_add(1);
        }
    }

    fn peek_raw(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos).map(|s| &s.token)
    }

    /// The next token that is not whitespace, not taken.
    #[must_use]
    pub fn peek(&self) -> Option<&'a Token> {
        let mut c = *self;
        c.skip_ws();
        c.peek_raw()
    }

    /// The next token that is not whitespace, taken.
    pub fn advance(&mut self) -> Option<&'a Token> {
        self.skip_ws();
        let t = self.peek_raw()?;
        self.pos = self.pos.saturating_add(1);
        Some(t)
    }

    /// Whether only whitespace is left.
    #[must_use]
    pub fn at_end(&self) -> bool {
        self.peek().is_none()
    }

    /// The tokens up to the `)` that closes the function or parenthesis just
    /// taken, that `)` taken too; `None` if it is never closed.
    pub fn take_block(&mut self) -> Option<&'a [Spanned]> {
        let start = self.pos;
        let mut depth = 0usize;
        while let Some(t) = self.peek_raw() {
            self.pos = self.pos.saturating_add(1);
            match t {
                Token::Function(_) | Token::OpenParen => depth = depth.saturating_add(1),
                Token::CloseParen if depth == 0 => {
                    return self.tokens.get(start..self.pos.saturating_sub(1));
                }
                Token::CloseParen => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }
}

/// Why a value was not read: said to whoever wrote the style.
pub type ValueError = String;

// ---------------------------------------------------------------------------
// Numbers, lengths and calc()
// ---------------------------------------------------------------------------

/// A value inside `calc()`: a plain number, or a length.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Calc {
    Number(f32),
    Length(Length),
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a style's numbers are read as f64 and used as f32, as every length in the toolkit is"
)]
fn narrow(v: f64) -> f32 {
    v as f32
}

/// One term of a `calc()` sum: a number, a length, a percentage, or a
/// parenthesised sum.
fn calc_value(c: &mut Cursor<'_>) -> Result<Calc, ValueError> {
    match c.advance() {
        Some(Token::Number(n)) => Ok(Calc::Number(narrow(*n))),
        Some(Token::Percentage(p)) => Ok(Calc::Length(Length {
            percent: narrow(*p),
            ..Length::default()
        })),
        Some(Token::Dimension(v, unit)) => length_in(narrow(*v), unit)
            .map(Calc::Length)
            .ok_or_else(|| format!("`{unit}` is not a unit this reads")),
        Some(Token::OpenParen) => {
            let inner = c.take_block().ok_or("a `(` is never closed")?;
            calc_sum(&mut Cursor::new(inner))
        }
        Some(Token::Function(name)) if name.eq_ignore_ascii_case("calc") => {
            let inner = c.take_block().ok_or("a `calc(` is never closed")?;
            calc_sum(&mut Cursor::new(inner))
        }
        Some(other) => Err(format!("{other:?} cannot be calculated with")),
        None => Err("a calculation ends where a value was expected".to_string()),
    }
}

fn calc_product(c: &mut Cursor<'_>) -> Result<Calc, ValueError> {
    let mut acc = calc_value(c)?;
    loop {
        match c.peek() {
            Some(Token::Delim('*')) => {
                c.advance();
                acc = match (acc, calc_value(c)?) {
                    (Calc::Number(a), Calc::Number(b)) => Calc::Number(a * b),
                    (Calc::Number(k), Calc::Length(l)) | (Calc::Length(l), Calc::Number(k)) => {
                        Calc::Length(l.scaled(k))
                    }
                    (Calc::Length(_), Calc::Length(_)) => {
                        return Err("a length times a length is not a length".to_string());
                    }
                };
            }
            Some(Token::Delim('/')) => {
                c.advance();
                let Calc::Number(k) = calc_value(c)? else {
                    return Err("a calculation may divide only by a number".to_string());
                };
                if k == 0.0 {
                    return Err("a calculation divides by nought".to_string());
                }
                acc = match acc {
                    Calc::Number(a) => Calc::Number(a / k),
                    Calc::Length(l) => Calc::Length(l.scaled(1.0 / k)),
                };
            }
            _ => return Ok(acc),
        }
    }
}

fn calc_sum(c: &mut Cursor<'_>) -> Result<Calc, ValueError> {
    let mut acc = calc_product(c)?;
    loop {
        let sign = match c.peek() {
            Some(Token::Delim('+')) => 1.0,
            Some(Token::Delim('-')) => -1.0,
            None => return Ok(acc),
            Some(other) => {
                return Err(format!(
                    "{other:?} where `+`, `-`, `*` or `/` was expected (CSS needs spaces round `+` and `-`)"
                ));
            }
        };
        c.advance();
        acc = match (acc, calc_product(c)?) {
            (Calc::Number(a), Calc::Number(b)) => Calc::Number(a + sign * b),
            (Calc::Length(a), Calc::Length(b)) => Calc::Length(a.plus(b.scaled(sign))),
            _ => return Err("a number and a length cannot be added".to_string()),
        };
    }
}

/// A `calc(...)` whose `calc(` has been taken.
fn calc(c: &mut Cursor<'_>) -> Result<Calc, ValueError> {
    let inner = c.take_block().ok_or("a `calc(` is never closed")?;
    let mut inner = Cursor::new(inner);
    let value = calc_sum(&mut inner)?;
    if inner.at_end() {
        Ok(value)
    } else {
        Err("a calculation has more after its end".to_string())
    }
}

/// A number: `1.5`, or a calculation that comes to one.
///
/// # Errors
///
/// Why the next value is not a number.
pub fn number(c: &mut Cursor<'_>) -> Result<f32, ValueError> {
    match c.advance() {
        Some(Token::Number(n)) => Ok(narrow(*n)),
        Some(Token::Function(name)) if name.eq_ignore_ascii_case("calc") => match calc(c)? {
            Calc::Number(n) => Ok(n),
            Calc::Length(_) => {
                Err("a calculation came to a length where a number was wanted".to_string())
            }
        },
        Some(other) => Err(format!("{other:?} where a number was wanted")),
        None => Err("a number was wanted".to_string()),
    }
}

/// A length, a percentage if `percent` allows one, or a calculation; a bare
/// `0` is a length too.
///
/// # Errors
///
/// Why the next value is not one.
pub fn length(c: &mut Cursor<'_>, percent: bool) -> Result<Length, ValueError> {
    let value = match c.advance() {
        Some(Token::Dimension(v, unit)) => length_in(narrow(*v), unit)
            .ok_or_else(|| format!("`{unit}` is not a unit this reads"))?,
        Some(Token::Percentage(p)) if percent => Length {
            percent: narrow(*p),
            ..Length::default()
        },
        Some(Token::Number(n)) if *n == 0.0 => Length::default(),
        Some(Token::Number(_)) => {
            return Err("a length needs a unit -- `px`, `em` -- unless it is 0".to_string());
        }
        Some(Token::Function(name)) if name.eq_ignore_ascii_case("calc") => match calc(c)? {
            Calc::Length(l) => l,
            Calc::Number(0.0) => Length::default(),
            Calc::Number(_) => {
                return Err("a calculation came to a number where a length was wanted".to_string());
            }
        },
        Some(other) => return Err(format!("{other:?} where a length was wanted")),
        None => return Err("a length was wanted".to_string()),
    };
    if !percent && value.has_percent() {
        return Err("a percentage is not a length this property takes".to_string());
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

/// A colour as written: one, or `currentcolor`, the text's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    /// This colour.
    Color(Color),
    /// The widget's text colour.
    CurrentColor,
}

/// A colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`,
/// `hsl()`, `hsla()`, a named colour, `transparent` or `currentcolor`.
///
/// # Errors
///
/// Why the next value is not one.
pub fn color(c: &mut Cursor<'_>) -> Result<ColorValue, ValueError> {
    match c.advance() {
        Some(Token::Hash(hex)) => hex_color(hex)
            .map(ColorValue::Color)
            .ok_or_else(|| format!("`#{hex}` is not a colour: 3, 4, 6 or 8 hex digits")),
        Some(Token::Ident(name)) => {
            let lower = name.to_ascii_lowercase();
            match lower.as_str() {
                "currentcolor" => Ok(ColorValue::CurrentColor),
                "transparent" => Ok(ColorValue::Color(Color::TRANSPARENT)),
                _ => named_color(&lower)
                    .map(ColorValue::Color)
                    .ok_or_else(|| format!("`{name}` is not a colour's name")),
            }
        }
        Some(Token::Function(name)) => {
            let lower = name.to_ascii_lowercase();
            let args = c
                .take_block()
                .ok_or_else(|| format!("`{name}(` is never closed"))?;
            match lower.as_str() {
                "rgb" | "rgba" => rgb_function(args).map(ColorValue::Color),
                "hsl" | "hsla" => hsl_function(args).map(ColorValue::Color),
                _ => Err(format!("`{name}()` is not a colour")),
            }
        }
        Some(other) => Err(format!("{other:?} where a colour was wanted")),
        None => Err("a colour was wanted".to_string()),
    }
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
#[must_use]
pub fn hex_color(hex: &str) -> Option<Color> {
    let digit = |c: char| c.to_digit(16).and_then(|d| u8::try_from(d).ok());
    let digits: Vec<u8> = hex.chars().map(digit).collect::<Option<_>>()?;
    let pair = |i: usize| -> Option<u8> {
        Some(
            digits
                .get(i)?
                .saturating_mul(16)
                .saturating_add(*digits.get(i.saturating_add(1))?),
        )
    };
    let single = |i: usize| -> Option<u8> { digits.get(i).map(|d| d.saturating_mul(17)) };
    match digits.len() {
        3 => Some(Color::rgba(single(0)?, single(1)?, single(2)?, 255)),
        4 => Some(Color::rgba(single(0)?, single(1)?, single(2)?, single(3)?)),
        6 => Some(Color::rgba(pair(0)?, pair(2)?, pair(4)?, 255)),
        8 => Some(Color::rgba(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
        _ => None,
    }
}

/// A function's arguments: either the legacy comma-separated list or the
/// modern space-separated one with an optional `/ alpha`.
fn arguments(args: &[Spanned]) -> Result<(Vec<Token>, Option<Token>), ValueError> {
    let mut c = Cursor::new(args);
    let mut values = Vec::new();
    let mut alpha = None;
    let commas = args.iter().any(|s| s.token == Token::Comma);
    while let Some(t) = c.advance() {
        match t {
            Token::Comma if commas => {}
            Token::Delim('/') if !commas => {
                alpha = Some(
                    c.advance()
                        .cloned()
                        .ok_or("an alpha was wanted after `/`")?,
                );
                if !c.at_end() {
                    return Err("more after the alpha".to_string());
                }
            }
            Token::Number(_) | Token::Percentage(_) | Token::Dimension(..) => {
                values.push(t.clone());
            }
            other => return Err(format!("{other:?} in a colour's arguments")),
        }
    }
    if commas && values.len() == 4 {
        alpha = values.pop();
    }
    if values.len() != 3 {
        return Err("a colour function takes three values and an alpha".to_string());
    }
    Ok((values, alpha))
}

/// An alpha: a number from 0 to 1, or a percentage.
fn alpha_of(t: Option<&Token>) -> Result<u8, ValueError> {
    let a = match t {
        None => 1.0,
        Some(Token::Number(n)) => narrow(*n),
        Some(Token::Percentage(p)) => narrow(*p) / 100.0,
        Some(other) => return Err(format!("{other:?} is not an alpha")),
    };
    Ok(channel(a.clamp(0.0, 1.0) * 255.0))
}

/// A channel from 0 to 255, rounded.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 and rounded first"
)]
fn channel(v: f32) -> u8 {
    if v.is_finite() {
        v.clamp(0.0, 255.0).round() as u8
    } else {
        0
    }
}

fn rgb_function(args: &[Spanned]) -> Result<Color, ValueError> {
    let (values, alpha) = arguments(args)?;
    let mut rgb = [0u8; 3];
    for (slot, v) in rgb.iter_mut().zip(&values) {
        *slot = match v {
            Token::Number(n) => channel(narrow(*n)),
            Token::Percentage(p) => channel(narrow(*p) * 2.55),
            other => return Err(format!("{other:?} is not a colour channel")),
        };
    }
    Ok(Color::rgba(
        rgb[0],
        rgb[1],
        rgb[2],
        alpha_of(alpha.as_ref())?,
    ))
}

fn hsl_function(args: &[Spanned]) -> Result<Color, ValueError> {
    let (values, alpha) = arguments(args)?;
    let hue = match values.first() {
        Some(Token::Number(n)) => narrow(*n),
        Some(Token::Dimension(v, unit)) => {
            let v = narrow(*v);
            match unit.to_ascii_lowercase().as_str() {
                "deg" => v,
                "rad" => v.to_degrees(),
                "grad" => v * 0.9,
                "turn" => v * 360.0,
                _ => return Err(format!("`{unit}` is not an angle")),
            }
        }
        other => return Err(format!("{other:?} is not a hue")),
    };
    let fraction = |t: Option<&Token>| match t {
        Some(Token::Percentage(p)) => Ok((narrow(*p) / 100.0).clamp(0.0, 1.0)),
        Some(Token::Number(n)) => Ok((narrow(*n) / 100.0).clamp(0.0, 1.0)),
        other => Err(format!("{other:?} is not a saturation or lightness")),
    };
    let s = fraction(values.get(1))?;
    let l = fraction(values.get(2))?;
    let (r, g, b) = hsl_to_rgb(hue, s, l);
    Ok(Color::rgba(
        channel(r * 255.0),
        channel(g * 255.0),
        channel(b * 255.0),
        alpha_of(alpha.as_ref())?,
    ))
}

/// CSS Color 4's `hsl()` to red, green and blue from 0 to 1.
fn hsl_to_rgb(hue: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let hue = hue.rem_euclid(360.0);
    let f = |n: f32| {
        let k = (n + hue / 30.0).rem_euclid(12.0);
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
}

/// CSS's named colours, by name: the 147 of CSS Color 4, sorted.
const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

/// The named colour `lower`, its name already in lower case.
#[must_use]
pub fn named_color(lower: &str) -> Option<Color> {
    let i = NAMED
        .binary_search_by(|(name, _)| (*name).cmp(lower))
        .ok()?;
    let (_, rgb) = NAMED.get(i)?;
    let [_, r, g, b] = rgb.to_be_bytes();
    Some(Color::rgba(r, g, b, 255))
}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;
