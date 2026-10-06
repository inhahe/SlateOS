//! Scalable cursors: a theme's `cursors_scalable` directory, as KDE Plasma
//! ships it -- for each cursor a folder of SVG pictures and a
//! `metadata.json` naming them -- drawn at exactly the size asked for, where
//! an XCursor file offers only the sizes it was drawn at.
//!
//! # The layout
//!
//! `cursors_scalable/<name>/metadata.json` is a list of frames, in the order
//! they show:
//!
//! ```json
//! [
//!     { "filename": "default.svg", "nominal_size": 24,
//!       "hotspot_x": 4, "hotspot_y": 4 },
//!     { "filename": "wait-02.svg", "nominal_size": 24,
//!       "hotspot_x": 12, "hotspot_y": 12, "delay": 30 }
//! ]
//! ```
//!
//! A frame's picture is its SVG at its own size -- `width` and `height`, or
//! its `viewBox`'s -- scaled by the size asked for over `nominal_size`; its hot
//! spot is scaled the same, and `delay` is how long it shows, in
//! milliseconds. That is KWin's reading (`SvgCursorReader`), so a theme drawn
//! for Plasma shows here as it does there.
//!
//! # Trust
//!
//! As for an XCursor file ([`super::xcursor`]): the metadata and each picture
//! are read only up to a bound, a frame names a file in its own folder and
//! nowhere else, a picture's sides are held to [`MAX_SIDE`], all the frames'
//! pixels together to [`MAX_DECODED_PIXELS`], and anything malformed is no
//! cursor -- the next place to look is tried.

use std::path::Path;

use guitk::svg::{SvgDocument, SvgNode};

use super::xcursor::{CursorFrame, CursorImages, MAX_DECODED_PIXELS, MAX_SIDE};

/// The directory inside a theme's folder that holds its scalable cursors.
pub const SCALABLE_DIR: &str = "cursors_scalable";

/// The file in a scalable cursor's folder that lists its frames.
pub const METADATA_FILE: &str = "metadata.json";

/// The largest `metadata.json` read: a few lines a frame, and a busy pointer
/// has sixty.
const MAX_METADATA_BYTES: u64 = 64 * 1024;

/// The largest SVG picture read for one frame.
const MAX_SVG_BYTES: u64 = 1024 * 1024;

/// The most frames one cursor may have.
const MAX_FRAMES: usize = 256;

/// The deepest JSON nesting read: the list, its objects, and room to spare.
const MAX_DEPTH: u32 = 8;

/// One frame as the metadata names it.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// The picture's file, in the cursor's folder.
    file: String,
    /// The size the picture was drawn for.
    nominal: f64,
    /// The hot spot, at the nominal size.
    hot: (f64, f64),
    /// How long it shows, in milliseconds.
    delay_ms: u32,
}

/// The scalable cursor in the folder `dir` -- a `cursors_scalable/<name>` --
/// drawn `size` pixels nominal; `None` for a folder that is not one, or
/// whose metadata or any picture cannot be read.
pub(super) fn read(dir: &Path, size: u32) -> Option<CursorImages> {
    if size == 0 || size > MAX_SIDE {
        return None;
    }
    let metadata = super::read_capped(&dir.join(METADATA_FILE), MAX_METADATA_BYTES)?;
    let entries = entries(std::str::from_utf8(&metadata).ok()?)?;
    let mut frames = Vec::with_capacity(entries.len());
    let mut pixels_so_far = 0usize;
    for entry in &entries {
        let frame = frame(dir, entry, size)?;
        pixels_so_far = pixels_so_far.checked_add(frame.pixels.len())?;
        if pixels_so_far > MAX_DECODED_PIXELS {
            return None;
        }
        frames.push(frame);
    }
    Some(CursorImages {
        nominal: size,
        frames,
    })
}

/// The frames `metadata` lists, in order; `None` for text that is not a
/// list of frames, an empty one, or one past [`MAX_FRAMES`].
fn entries(metadata: &str) -> Option<Vec<Entry>> {
    let Json::Array(items) = parse(metadata)? else {
        return None;
    };
    if items.is_empty() || items.len() > MAX_FRAMES {
        return None;
    }
    items.iter().map(entry).collect()
}

/// One frame from its object in the metadata: a file and a positive
/// nominal size it must have; a hot spot and a delay it may.
fn entry(item: &Json) -> Option<Entry> {
    let Json::Object(fields) = item else {
        return None;
    };
    let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let number = |key: &str| match field(key) {
        Some(Json::Number(n)) if n.is_finite() => Some(Some(*n)),
        None => Some(None),
        Some(_) => None,
    };
    let Some(Json::Text(file)) = field("filename") else {
        return None;
    };
    let nominal = number("nominal_size")?.filter(|n| *n > 0.0)?;
    let hot = (
        number("hotspot_x")?.unwrap_or(0.0),
        number("hotspot_y")?.unwrap_or(0.0),
    );
    let delay = number("delay")?.unwrap_or(0.0);
    if !is_plain_file_name(file) || hot.0 < 0.0 || hot.1 < 0.0 || delay < 0.0 {
        return None;
    }
    Some(Entry {
        file: file.clone(),
        nominal,
        hot,
        // Held to a minute: a frame that shows longer than that is a
        // mistake, and the cast below needs a bound.
        delay_ms: whole(delay.min(60_000.0)),
    })
}

/// Whether `name` names a file in the cursor's own folder: no separator, not
/// `.` or `..`, not empty.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.contains(':')
}

/// A non-negative number, rounded, as a `u32` -- at most `u32::MAX`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a whole number within u32's range, by the two arms before the cast"
)]
fn whole(n: f64) -> u32 {
    let rounded = n.round();
    if rounded <= 0.0 {
        0
    } else if rounded >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        rounded as u32
    }
}

/// The frame `entry` names, drawn for a cursor `size` pixels nominal.
fn frame(dir: &Path, entry: &Entry, size: u32) -> Option<CursorFrame> {
    let bytes = super::read_capped(&dir.join(&entry.file), MAX_SVG_BYTES)?;
    let doc = SvgDocument::parse(std::str::from_utf8(&bytes).ok()?).ok()?;
    let (own_w, own_h) = own_size(&doc);
    let scale = f64::from(size) / entry.nominal;
    let side = |n: f32| {
        let scaled = whole(f64::from(n) * scale);
        (1..=MAX_SIDE).contains(&scaled).then_some(scaled)
    };
    let (width, height) = (side(own_w)?, side(own_h)?);
    let rgba = doc.render(width, height);
    let pixels: Vec<u32> = rgba
        .chunks_exact(4)
        .map(|p| match *p {
            [r, g, b, a] => premultiplied(r, g, b, a),
            _ => 0,
        })
        .collect();
    let expected = usize::try_from(u64::from(width).saturating_mul(u64::from(height))).ok()?;
    if pixels.len() != expected {
        return None;
    }
    Some(CursorFrame {
        width,
        height,
        // On the picture, as an XCursor file's must be.
        hot_x: whole(entry.hot.0 * scale).min(width.saturating_sub(1)),
        hot_y: whole(entry.hot.1 * scale).min(height.saturating_sub(1)),
        delay_ms: entry.delay_ms,
        pixels,
    })
}

/// An SVG picture's own size: its `width` and `height` where it says them,
/// else its `viewBox`'s -- what an SVG viewer shows it at, and what a scalable
/// cursor's nominal size was measured against.
fn own_size(doc: &SvgDocument) -> (f32, f32) {
    let (_, _, box_w, box_h) = doc.viewbox();
    match &doc.root {
        SvgNode::Svg { width, height, .. } => (width.unwrap_or(box_w), height.unwrap_or(box_h)),
        _ => (box_w, box_h),
    }
}

/// A straight-alpha pixel as premultiplied `0xAARRGGBB`, as cursor pixels
/// are kept.
fn premultiplied(r: u8, g: u8, b: u8, a: u8) -> u32 {
    let times = |c: u8| {
        // `c * a / 255`, rounded: both at most 255, so the product fits and
        // the quotient is a byte again.
        let product = u16::from(c).saturating_mul(u16::from(a));
        u8::try_from(product.saturating_add(127) / 255).unwrap_or(u8::MAX)
    };
    u32::from_be_bytes([a, times(r), times(g), times(b)])
}

// ---------------------------------------------------------------------------
// JSON, as much as a cursor's metadata needs
// ---------------------------------------------------------------------------

/// A JSON value: the whole grammar (RFC 8259), kept small -- what a cursor's
/// metadata file is read with. Numbers are `f64`, as JSON's are.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// The JSON value `text` holds, with nothing after it but white space; `None`
/// for text that is not JSON, or nests past [`MAX_DEPTH`].
fn parse(text: &str) -> Option<Json> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value(0)?;
    parser.skip_space();
    (parser.at == parser.bytes.len()).then_some(value)
}

/// A reading position in JSON text.
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    /// The byte at the position, if any.
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// Past the byte at the position.
    fn bump(&mut self) {
        self.at = self.at.saturating_add(1);
    }

    /// Past JSON's white space.
    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    /// Past `byte`, which must be next (after white space).
    fn expect(&mut self, byte: u8) -> Option<()> {
        self.skip_space();
        (self.peek() == Some(byte)).then(|| self.bump())
    }

    /// The value at the position, `depth` levels down.
    fn value(&mut self, depth: u32) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.skip_space();
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(Json::Text),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    /// The word `word`, which must be next, as `value`.
    fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
        let end = self.at.checked_add(word.len())?;
        if self.bytes.get(self.at..end)? != word.as_bytes() {
            return None;
        }
        self.at = end;
        Some(value)
    }

    /// The number at the position, by JSON's grammar.
    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        // An integer part: `0`, or a digit from 1 followed by digits.
        match self.peek()? {
            b'0' => self.bump(),
            b'1'..=b'9' => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.bump();
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            self.digits()?;
        }
        let text = std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
        text.parse::<f64>().ok().map(Json::Number)
    }

    /// Past one digit or more, which must be next.
    fn digits(&mut self) -> Option<()> {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        (self.at > start).then_some(())
    }

    /// The string at the position, its escapes undone.
    fn string(&mut self) -> Option<String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = self.peek()?;
            self.bump();
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.bump();
                    let ch = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                // A control character must be escaped in JSON.
                0..=0x1f => return None,
                other => out.push(other),
            }
        }
    }

    /// The character a `\u` escape names -- the four hex digits after it, and
    /// a second escape after a high surrogate.
    fn unicode_escape(&mut self) -> Option<char> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            // A high surrogate must be followed by a low one.
            self.literal("\\u", Json::Null)?;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return None;
            }
            // Both in their ranges, so neither subtraction wraps and the sum
            // is at most 0x10FFFF.
            let high = first.saturating_sub(0xD800) << 10;
            let code = 0x10000u32
                .saturating_add(high)
                .saturating_add(second.saturating_sub(0xDC00));
            return char::from_u32(code);
        }
        char::from_u32(first)
    }

    /// The four hex digits at the position, as a number.
    fn hex4(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let digits = std::str::from_utf8(self.bytes.get(self.at..end)?).ok()?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at = end;
        u32::from_str_radix(digits, 16).ok()
    }

    /// The array at the position.
    fn array(&mut self, depth: u32) -> Option<Json> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.bump();
            return Some(Json::Array(items));
        }
        loop {
            items.push(self.value(depth.saturating_add(1))?);
            self.skip_space();
            match self.peek()? {
                b',' => self.bump(),
                b']' => {
                    self.bump();
                    return Some(Json::Array(items));
                }
                _ => return None,
            }
        }
    }

    /// The object at the position, its fields in order.
    fn object(&mut self, depth: u32) -> Option<Json> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.bump();
            return Some(Json::Object(fields));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.expect(b':')?;
            let value = self.value(depth.saturating_add(1))?;
            fields.push((key, value));
            self.skip_space();
            match self.peek()? {
                b',' => self.bump(),
                b'}' => {
                    self.bump();
                    return Some(Json::Object(fields));
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
#[path = "scalable_tests.rs"]
mod tests;
