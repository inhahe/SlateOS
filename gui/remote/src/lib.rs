//! Slate OS native remote-desktop protocol.
//!
//! The compositor in Slate OS already drives the screen from a stream of
//! [`guitk::render::RenderCommand`]s — high-level draw primitives (rect
//! fills, text, lines, clips, transforms). For remote display we serialise
//! that same command stream and ship it to a remote viewer, which decodes
//! the bytes back into a `RenderTree` and re-runs them through its local
//! compositor or framebuffer. No pixel encoding, no video codec — the
//! viewer just replays the draw commands.
//!
//! This is the most efficient remote-desktop option when the compositor
//! knows the draw commands: a typical UI frame is hundreds of bytes of
//! commands versus megabytes of pixels. Latency is determined by the
//! network round trip, not by an encoder. Text remains pixel-perfect.
//!
//! ## Frame format
//!
//! Each frame is a self-contained, length-prefixed envelope:
//!
//! ```text
//! +--------+------+--------+----------+----------+-----+----------+
//! | magic  | ver  | flags  | n_cmds   |  cmd_0   | ... |  cmd_n   |
//! | 4 B    | 1 B  | 1 B    | 4 B LE   |  variable             ... |
//! +--------+------+--------+----------+----------+-----+----------+
//! ```
//!
//! * **magic** = `b"ORDR"` (0x4F 0x52 0x44 0x52 — "Slate OS Render")
//! * **ver** = [`PROTOCOL_VERSION`]
//! * **flags** = reserved, must be zero
//! * **n_cmds** = number of render commands in this frame, little-endian u32
//!
//! Each command starts with a [`Tag`] byte followed by a fixed payload.
//! All scalars are little-endian; `f32` is encoded by `to_le_bytes` of the
//! IEEE-754 bit pattern. Strings are length-prefixed (u32 LE) UTF-8.
//!
//! ## Streaming
//!
//! [`decode_frame`] consumes exactly one frame and returns the number of
//! bytes it read. [`try_decode_frame`] is the streaming version: it
//! returns `Ok(None)` when the buffer holds only a partial frame so the
//! caller can keep reading from its transport.
//!
//! ## Robustness
//!
//! The decoder never panics on malformed or truncated input. All payload
//! reads are bounds-checked; invalid tag bytes, oversized lengths, and
//! malformed UTF-8 are reported as [`DecodeError`].

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::doc_markdown
)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

// ============================================================================
// Protocol constants
// ============================================================================

/// Frame magic: `b"ORDR"`.
pub const MAGIC: [u8; 4] = *b"ORDR";

/// Current protocol version. Increment on any breaking change to the wire
/// format; never reuse a version number.
pub const PROTOCOL_VERSION: u8 = 1;

/// Frame header size (magic + version + flags + cmd-count).
const HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// Maximum command count per frame. Frames larger than this are rejected to
/// limit memory consumption from a hostile sender; a real compositor frame
/// has on the order of 10^3 commands.
pub const MAX_COMMANDS_PER_FRAME: u32 = 1 << 20;

/// Maximum length of a single string field (text, image references, etc.).
/// 4 MiB is generous for any realistic UI text run while still bounding
/// peak memory.
pub const MAX_STRING_LEN: u32 = 4 * 1024 * 1024;

// ============================================================================
// Command tags
// ============================================================================

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    FillRect = 0x01,
    StrokeRect = 0x02,
    Text = 0x03,
    Image = 0x04,
    Line = 0x05,
    PushClip = 0x06,
    PopClip = 0x07,
    PushTranslate = 0x08,
    PopTranslate = 0x09,
    BoxShadow = 0x0A,
}

impl Tag {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::FillRect),
            0x02 => Some(Self::StrokeRect),
            0x03 => Some(Self::Text),
            0x04 => Some(Self::Image),
            0x05 => Some(Self::Line),
            0x06 => Some(Self::PushClip),
            0x07 => Some(Self::PopClip),
            0x08 => Some(Self::PushTranslate),
            0x09 => Some(Self::PopTranslate),
            0x0A => Some(Self::BoxShadow),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FontWeightTag {
    Regular = 0x00,
    Bold = 0x01,
    Light = 0x02,
}

impl FontWeightTag {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Regular),
            0x01 => Some(Self::Bold),
            0x02 => Some(Self::Light),
            _ => None,
        }
    }

    fn to_weight(self) -> FontWeightHint {
        match self {
            Self::Regular => FontWeightHint::Regular,
            Self::Bold => FontWeightHint::Bold,
            Self::Light => FontWeightHint::Light,
        }
    }

    fn from_weight(w: FontWeightHint) -> Self {
        match w {
            FontWeightHint::Regular => Self::Regular,
            FontWeightHint::Bold => Self::Bold,
            FontWeightHint::Light => Self::Light,
        }
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Errors returned by the decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Input ended in the middle of a frame.
    UnexpectedEof,
    /// Frame magic did not match `MAGIC`.
    BadMagic,
    /// Protocol version is not supported by this decoder.
    UnsupportedVersion(u8),
    /// `flags` byte was non-zero (reserved bits set).
    ReservedFlags(u8),
    /// `n_cmds` exceeds [`MAX_COMMANDS_PER_FRAME`].
    TooManyCommands(u32),
    /// String length exceeds [`MAX_STRING_LEN`].
    StringTooLarge(u32),
    /// Encountered a tag byte that does not correspond to a known command.
    BadTag(u8),
    /// A `FontWeightHint` tag byte was unknown.
    BadFontWeight(u8),
    /// A string field was not valid UTF-8.
    BadUtf8,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
            Self::BadMagic => write!(f, "invalid frame magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported protocol version {v}"),
            Self::ReservedFlags(b) => write!(f, "reserved flags bits set: {b:#04x}"),
            Self::TooManyCommands(n) => {
                write!(f, "command count {n} exceeds limit {MAX_COMMANDS_PER_FRAME}")
            }
            Self::StringTooLarge(n) => {
                write!(f, "string length {n} exceeds limit {MAX_STRING_LEN}")
            }
            Self::BadTag(b) => write!(f, "unknown command tag {b:#04x}"),
            Self::BadFontWeight(b) => write!(f, "unknown font-weight tag {b:#04x}"),
            Self::BadUtf8 => write!(f, "string field was not valid UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

// ============================================================================
// Encoding
// ============================================================================

/// Append a single encoded frame containing `tree`'s commands to `out`.
///
/// The encoder reserves the header bytes up front, writes each command,
/// then back-fills the command count. This avoids a pre-pass to count
/// commands and keeps allocations to a single growable buffer.
pub fn encode_frame(tree: &RenderTree, out: &mut Vec<u8>) {
    let header_pos = out.len();
    // Reserve header.
    out.extend_from_slice(&MAGIC);
    out.push(PROTOCOL_VERSION);
    out.push(0); // flags
    out.extend_from_slice(&0u32.to_le_bytes()); // n_cmds placeholder

    let mut count: u32 = 0;
    for cmd in &tree.commands {
        encode_command(cmd, out);
        count = count.saturating_add(1);
    }

    // Back-fill n_cmds at header_pos + 6.
    let count_offset = header_pos + 4 + 1 + 1;
    let bytes = count.to_le_bytes();
    out[count_offset] = bytes[0];
    out[count_offset + 1] = bytes[1];
    out[count_offset + 2] = bytes[2];
    out[count_offset + 3] = bytes[3];
}

/// Convenience: encode a frame and return the bytes.
#[must_use]
pub fn encode_frame_to_vec(tree: &RenderTree) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER_LEN + tree.len() * 32);
    encode_frame(tree, &mut v);
    v
}

fn encode_command(cmd: &RenderCommand, out: &mut Vec<u8>) {
    match cmd {
        RenderCommand::FillRect { x, y, width, height, color, corner_radii } => {
            out.push(Tag::FillRect as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_f32(out, *width);
            write_f32(out, *height);
            write_color(out, *color);
            write_radii(out, *corner_radii);
        }
        RenderCommand::StrokeRect { x, y, width, height, color, line_width, corner_radii } => {
            out.push(Tag::StrokeRect as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_f32(out, *width);
            write_f32(out, *height);
            write_color(out, *color);
            write_f32(out, *line_width);
            write_radii(out, *corner_radii);
        }
        RenderCommand::Text { x, y, text, color, font_size, font_weight, max_width } => {
            out.push(Tag::Text as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_string(out, text);
            write_color(out, *color);
            write_f32(out, *font_size);
            out.push(FontWeightTag::from_weight(*font_weight) as u8);
            write_optional_f32(out, *max_width);
        }
        RenderCommand::Image { x, y, width, height, image_id } => {
            out.push(Tag::Image as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_f32(out, *width);
            write_f32(out, *height);
            write_u64(out, *image_id);
        }
        RenderCommand::Line { x1, y1, x2, y2, color, width } => {
            out.push(Tag::Line as u8);
            write_f32(out, *x1);
            write_f32(out, *y1);
            write_f32(out, *x2);
            write_f32(out, *y2);
            write_color(out, *color);
            write_f32(out, *width);
        }
        RenderCommand::PushClip { x, y, width, height } => {
            out.push(Tag::PushClip as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_f32(out, *width);
            write_f32(out, *height);
        }
        RenderCommand::PopClip => {
            out.push(Tag::PopClip as u8);
        }
        RenderCommand::PushTranslate { dx, dy } => {
            out.push(Tag::PushTranslate as u8);
            write_f32(out, *dx);
            write_f32(out, *dy);
        }
        RenderCommand::PopTranslate => {
            out.push(Tag::PopTranslate as u8);
        }
        RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x,
            offset_y,
            blur,
            spread,
            color,
            corner_radii,
        } => {
            out.push(Tag::BoxShadow as u8);
            write_f32(out, *x);
            write_f32(out, *y);
            write_f32(out, *width);
            write_f32(out, *height);
            write_f32(out, *offset_x);
            write_f32(out, *offset_y);
            write_f32(out, *blur);
            write_f32(out, *spread);
            write_color(out, *color);
            write_radii(out, *corner_radii);
        }
    }
}

fn write_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_color(out: &mut Vec<u8>, c: Color) {
    out.push(c.r);
    out.push(c.g);
    out.push(c.b);
    out.push(c.a);
}

fn write_radii(out: &mut Vec<u8>, r: CornerRadii) {
    write_f32(out, r.top_left);
    write_f32(out, r.top_right);
    write_f32(out, r.bottom_right);
    write_f32(out, r.bottom_left);
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    // SAFETY: a string's byte length always fits in u32 unless it is larger
    // than 4 GiB, which violates MAX_STRING_LEN on decode anyway. We
    // saturate on encode to avoid silent truncation.
    let bytes = s.as_bytes();
    let len_u32 = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    write_u32(out, len_u32);
    out.extend_from_slice(bytes);
}

fn write_optional_f32(out: &mut Vec<u8>, v: Option<f32>) {
    match v {
        Some(x) => {
            out.push(1);
            write_f32(out, x);
        }
        None => {
            out.push(0);
        }
    }
}

// ============================================================================
// Decoding
// ============================================================================

/// Reader cursor over a byte slice, with bounds-checked primitive reads.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn need(&self, n: usize) -> Result<(), DecodeError> {
        if self.remaining() < n {
            Err(DecodeError::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        self.need(1)?;
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        self.need(4)?;
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_u64(&mut self) -> Result<u64, DecodeError> {
        self.need(8)?;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.buf[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_f32(&mut self) -> Result<f32, DecodeError> {
        self.need(4)?;
        let bits = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(f32::from_bits(bits))
    }

    fn read_color(&mut self) -> Result<Color, DecodeError> {
        self.need(4)?;
        let c = Color {
            r: self.buf[self.pos],
            g: self.buf[self.pos + 1],
            b: self.buf[self.pos + 2],
            a: self.buf[self.pos + 3],
        };
        self.pos += 4;
        Ok(c)
    }

    fn read_radii(&mut self) -> Result<CornerRadii, DecodeError> {
        let tl = self.read_f32()?;
        let tr = self.read_f32()?;
        let br = self.read_f32()?;
        let bl = self.read_f32()?;
        Ok(CornerRadii {
            top_left: tl,
            top_right: tr,
            bottom_right: br,
            bottom_left: bl,
        })
    }

    fn read_string(&mut self) -> Result<String, DecodeError> {
        let len = self.read_u32()?;
        if len > MAX_STRING_LEN {
            return Err(DecodeError::StringTooLarge(len));
        }
        let len_usize = len as usize;
        self.need(len_usize)?;
        let slice = &self.buf[self.pos..self.pos + len_usize];
        let s = core::str::from_utf8(slice)
            .map_err(|_| DecodeError::BadUtf8)?
            .to_string();
        self.pos += len_usize;
        Ok(s)
    }

    fn read_optional_f32(&mut self) -> Result<Option<f32>, DecodeError> {
        let tag = self.read_u8()?;
        match tag {
            0 => Ok(None),
            1 => Ok(Some(self.read_f32()?)),
            other => Err(DecodeError::BadTag(other)),
        }
    }
}

/// Decode exactly one frame from `input`. Returns the decoded tree and the
/// number of bytes consumed.
pub fn decode_frame(input: &[u8]) -> Result<(RenderTree, usize), DecodeError> {
    let (tree, consumed) = decode_internal(input)?;
    Ok((tree, consumed))
}

/// Streaming-friendly decode: returns `Ok(None)` when the buffer holds only
/// part of a frame, leaving the caller free to keep reading from its
/// transport. Returns `Err` only for genuine corruption (bad magic, bad
/// version, bad tag), not for short reads.
pub fn try_decode_frame(input: &[u8]) -> Result<Option<(RenderTree, usize)>, DecodeError> {
    match decode_internal(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

fn decode_internal(input: &[u8]) -> Result<(RenderTree, usize), DecodeError> {
    let mut r = Reader::new(input);
    // Header.
    r.need(HEADER_LEN)?;
    let magic = [
        r.buf[0],
        r.buf[1],
        r.buf[2],
        r.buf[3],
    ];
    if magic != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    r.pos = 4;
    let ver = r.read_u8()?;
    if ver != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let n_cmds = r.read_u32()?;
    if n_cmds > MAX_COMMANDS_PER_FRAME {
        return Err(DecodeError::TooManyCommands(n_cmds));
    }
    let mut tree = RenderTree::with_capacity(n_cmds as usize);
    for _ in 0..n_cmds {
        let cmd = decode_command(&mut r)?;
        tree.push(cmd);
    }
    Ok((tree, r.pos))
}

fn decode_command(r: &mut Reader<'_>) -> Result<RenderCommand, DecodeError> {
    let tag_byte = r.read_u8()?;
    let tag = Tag::from_byte(tag_byte).ok_or(DecodeError::BadTag(tag_byte))?;
    Ok(match tag {
        Tag::FillRect => RenderCommand::FillRect {
            x: r.read_f32()?,
            y: r.read_f32()?,
            width: r.read_f32()?,
            height: r.read_f32()?,
            color: r.read_color()?,
            corner_radii: r.read_radii()?,
        },
        Tag::StrokeRect => RenderCommand::StrokeRect {
            x: r.read_f32()?,
            y: r.read_f32()?,
            width: r.read_f32()?,
            height: r.read_f32()?,
            color: r.read_color()?,
            line_width: r.read_f32()?,
            corner_radii: r.read_radii()?,
        },
        Tag::Text => {
            let x = r.read_f32()?;
            let y = r.read_f32()?;
            let text = r.read_string()?;
            let color = r.read_color()?;
            let font_size = r.read_f32()?;
            let weight_byte = r.read_u8()?;
            let font_weight = FontWeightTag::from_byte(weight_byte)
                .ok_or(DecodeError::BadFontWeight(weight_byte))?
                .to_weight();
            let max_width = r.read_optional_f32()?;
            RenderCommand::Text {
                x,
                y,
                text,
                color,
                font_size,
                font_weight,
                max_width,
            }
        }
        Tag::Image => RenderCommand::Image {
            x: r.read_f32()?,
            y: r.read_f32()?,
            width: r.read_f32()?,
            height: r.read_f32()?,
            image_id: r.read_u64()?,
        },
        Tag::Line => RenderCommand::Line {
            x1: r.read_f32()?,
            y1: r.read_f32()?,
            x2: r.read_f32()?,
            y2: r.read_f32()?,
            color: r.read_color()?,
            width: r.read_f32()?,
        },
        Tag::PushClip => RenderCommand::PushClip {
            x: r.read_f32()?,
            y: r.read_f32()?,
            width: r.read_f32()?,
            height: r.read_f32()?,
        },
        Tag::PopClip => RenderCommand::PopClip,
        Tag::PushTranslate => RenderCommand::PushTranslate {
            dx: r.read_f32()?,
            dy: r.read_f32()?,
        },
        Tag::PopTranslate => RenderCommand::PopTranslate,
        Tag::BoxShadow => RenderCommand::BoxShadow {
            x: r.read_f32()?,
            y: r.read_f32()?,
            width: r.read_f32()?,
            height: r.read_f32()?,
            offset_x: r.read_f32()?,
            offset_y: r.read_f32()?,
            blur: r.read_f32()?,
            spread: r.read_f32()?,
            color: r.read_color()?,
            corner_radii: r.read_radii()?,
        },
    })
}

// Local extension on RenderTree to construct with capacity without modifying
// the toolkit crate.
trait RenderTreeWithCapacity {
    fn with_capacity(n: usize) -> Self;
}

impl RenderTreeWithCapacity for RenderTree {
    fn with_capacity(n: usize) -> Self {
        // RenderTree::new() is fine; capacity is best-effort.
        let mut t = Self::new();
        t.commands.reserve(n.min(MAX_COMMANDS_PER_FRAME as usize));
        t
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::color::Color;
    use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
    use guitk::style::CornerRadii;

    fn radii(a: f32) -> CornerRadii {
        CornerRadii {
            top_left: a,
            top_right: a + 1.0,
            bottom_right: a + 2.0,
            bottom_left: a + 3.0,
        }
    }

    fn sample_tree() -> RenderTree {
        let mut t = RenderTree::new();
        t.commands.push(RenderCommand::FillRect {
            x: 1.0,
            y: 2.0,
            width: 100.0,
            height: 50.0,
            color: Color::rgba(10, 20, 30, 40),
            corner_radii: radii(4.0),
        });
        t.commands.push(RenderCommand::StrokeRect {
            x: 5.0,
            y: 6.0,
            width: 80.0,
            height: 40.0,
            color: Color::rgba(50, 60, 70, 255),
            line_width: 1.5,
            corner_radii: CornerRadii::ZERO,
        });
        t.commands.push(RenderCommand::Text {
            x: 10.0,
            y: 20.0,
            text: "Hello, Slate OS!".to_string(),
            color: Color::rgb(255, 255, 255),
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
        });
        t.commands.push(RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: "no wrap".to_string(),
            color: Color::rgb(1, 2, 3),
            font_size: 12.0,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
        t.commands.push(RenderCommand::Image {
            x: 30.0,
            y: 40.0,
            width: 64.0,
            height: 64.0,
            image_id: 0xDEAD_BEEF_CAFE_F00D,
        });
        t.commands.push(RenderCommand::Line {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
            color: Color::rgb(0, 128, 255),
            width: 2.0,
        });
        t.commands.push(RenderCommand::PushClip {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        });
        t.commands.push(RenderCommand::PushTranslate { dx: 10.0, dy: -5.0 });
        t.commands.push(RenderCommand::PopTranslate);
        t.commands.push(RenderCommand::PopClip);
        t.commands.push(RenderCommand::BoxShadow {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 1.0,
            color: Color::rgba(0, 0, 0, 128),
            corner_radii: radii(6.0),
        });
        t
    }

    fn assert_command_eq(a: &RenderCommand, b: &RenderCommand) {
        // We can't derive PartialEq on RenderCommand (floats), so compare
        // their debug strings — robust enough for round-trip tests.
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    fn assert_tree_eq(a: &RenderTree, b: &RenderTree) {
        assert_eq!(a.commands.len(), b.commands.len(), "command count differs");
        for (i, (x, y)) in a.commands.iter().zip(b.commands.iter()).enumerate() {
            assert_eq!(
                format!("{x:?}"),
                format!("{y:?}"),
                "command #{i} differs"
            );
        }
    }

    #[test]
    fn empty_frame_roundtrip() {
        let t = RenderTree::new();
        let bytes = encode_frame_to_vec(&t);
        assert_eq!(bytes.len(), HEADER_LEN);
        let (decoded, consumed) = decode_frame(&bytes).unwrap();
        assert_eq!(consumed, bytes.len());
        assert!(decoded.commands.is_empty());
    }

    #[test]
    fn full_frame_roundtrip() {
        let t = sample_tree();
        let bytes = encode_frame_to_vec(&t);
        let (decoded, consumed) = decode_frame(&bytes).unwrap();
        assert_eq!(consumed, bytes.len());
        assert_tree_eq(&t, &decoded);
    }

    #[test]
    fn header_layout_is_stable() {
        let t = RenderTree::new();
        let bytes = encode_frame_to_vec(&t);
        assert_eq!(&bytes[..4], &MAGIC);
        assert_eq!(bytes[4], PROTOCOL_VERSION);
        assert_eq!(bytes[5], 0);
        assert_eq!(&bytes[6..10], &0u32.to_le_bytes());
    }

    #[test]
    fn each_command_kind_roundtrips_individually() {
        let cmds = sample_tree().commands;
        for cmd in &cmds {
            let mut t = RenderTree::new();
            t.commands.push(cmd.clone());
            let bytes = encode_frame_to_vec(&t);
            let (decoded, _) = decode_frame(&bytes).unwrap();
            assert_eq!(decoded.commands.len(), 1);
            assert_command_eq(cmd, &decoded.commands[0]);
        }
    }

    #[test]
    fn streaming_two_frames_back_to_back() {
        let t1 = sample_tree();
        let t2 = {
            let mut t = RenderTree::new();
            t.commands.push(RenderCommand::PushTranslate { dx: 1.0, dy: 2.0 });
            t.commands.push(RenderCommand::PopTranslate);
            t
        };
        let mut buf = encode_frame_to_vec(&t1);
        let mut second = encode_frame_to_vec(&t2);
        buf.append(&mut second);
        // First frame.
        let (d1, c1) = try_decode_frame(&buf).unwrap().unwrap();
        assert_tree_eq(&t1, &d1);
        // Second frame from the remainder.
        let (d2, c2) = try_decode_frame(&buf[c1..]).unwrap().unwrap();
        assert_tree_eq(&t2, &d2);
        assert_eq!(c1 + c2, buf.len());
    }

    #[test]
    fn try_decode_returns_none_on_partial_frame() {
        let t = sample_tree();
        let bytes = encode_frame_to_vec(&t);
        // Truncate at every prefix length and confirm we get None (not Err).
        for cut in 0..bytes.len() {
            let result = try_decode_frame(&bytes[..cut]).unwrap();
            assert!(result.is_none(), "got Some at prefix len {cut}");
        }
        // Full input decodes.
        assert!(try_decode_frame(&bytes).unwrap().is_some());
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = encode_frame_to_vec(&RenderTree::new());
        bytes[0] = 0xFF;
        assert!(matches!(decode_frame(&bytes), Err(DecodeError::BadMagic)));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let mut bytes = encode_frame_to_vec(&RenderTree::new());
        bytes[4] = PROTOCOL_VERSION.wrapping_add(7);
        assert!(matches!(
            decode_frame(&bytes),
            Err(DecodeError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn reserved_flags_are_rejected() {
        let mut bytes = encode_frame_to_vec(&RenderTree::new());
        bytes[5] = 0x01;
        assert!(matches!(decode_frame(&bytes), Err(DecodeError::ReservedFlags(_))));
    }

    #[test]
    fn unknown_command_tag_is_rejected() {
        let mut t = RenderTree::new();
        t.commands.push(RenderCommand::PopClip);
        let mut bytes = encode_frame_to_vec(&t);
        // Header is 10 bytes; the next byte is the tag. Flip it to invalid.
        bytes[HEADER_LEN] = 0xFE;
        assert!(matches!(decode_frame(&bytes), Err(DecodeError::BadTag(0xFE))));
    }

    #[test]
    fn truncated_in_middle_of_command_is_eof() {
        let t = sample_tree();
        let mut bytes = encode_frame_to_vec(&t);
        // Drop the last 4 bytes — should be inside the final BoxShadow.
        bytes.truncate(bytes.len() - 4);
        assert!(matches!(decode_frame(&bytes), Err(DecodeError::UnexpectedEof)));
    }

    #[test]
    fn oversized_string_is_rejected() {
        // Hand-craft a Text command with len > MAX_STRING_LEN.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(0); // flags
        bytes.extend_from_slice(&1u32.to_le_bytes()); // n_cmds
        bytes.push(Tag::Text as u8);
        bytes.extend_from_slice(&0f32.to_le_bytes()); // x
        bytes.extend_from_slice(&0f32.to_le_bytes()); // y
        bytes.extend_from_slice(&(MAX_STRING_LEN + 1).to_le_bytes()); // bad len
        // The rest never gets read.
        assert!(matches!(
            decode_frame(&bytes),
            Err(DecodeError::StringTooLarge(_))
        ));
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn bad_utf8_string_is_rejected() {
        // Hand-craft a Text command with invalid UTF-8 bytes.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(Tag::Text as u8);
        bytes.extend_from_slice(&0f32.to_le_bytes()); // x
        bytes.extend_from_slice(&0f32.to_le_bytes()); // y
        // 2-byte payload that's not valid UTF-8 (lone continuation byte).
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.push(0xFF);
        bytes.push(0xFE);
        // remaining color (4) + font_size (4) + weight (1) + max_width tag (1)
        bytes.extend_from_slice(&[0, 0, 0, 255]);
        bytes.extend_from_slice(&12f32.to_le_bytes());
        bytes.push(0);
        bytes.push(0);
        assert!(matches!(decode_frame(&bytes), Err(DecodeError::BadUtf8)));
    }

    #[test]
    fn empty_input_is_eof_for_decode_but_none_for_try() {
        assert!(matches!(decode_frame(&[]), Err(DecodeError::UnexpectedEof)));
        assert!(try_decode_frame(&[]).unwrap().is_none());
    }

    #[test]
    fn too_many_commands_is_rejected_before_allocation() {
        // Craft a header claiming a billion commands.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(0);
        let huge: u32 = MAX_COMMANDS_PER_FRAME + 1;
        bytes.extend_from_slice(&huge.to_le_bytes());
        assert!(matches!(
            decode_frame(&bytes),
            Err(DecodeError::TooManyCommands(_))
        ));
    }

    #[test]
    fn empty_text_string_roundtrips() {
        let mut t = RenderTree::new();
        t.commands.push(RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: String::new(),
            color: Color::rgb(0, 0, 0),
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        let bytes = encode_frame_to_vec(&t);
        let (d, _) = decode_frame(&bytes).unwrap();
        assert_eq!(d.commands.len(), 1);
        if let RenderCommand::Text { text, .. } = &d.commands[0] {
            assert!(text.is_empty());
        } else {
            panic!("expected Text command");
        }
    }

    #[test]
    fn non_ascii_text_roundtrips() {
        let original = "日本語 — \u{1F600} 🎨";
        let mut t = RenderTree::new();
        t.commands.push(RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: original.to_string(),
            color: Color::rgb(0, 0, 0),
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        let bytes = encode_frame_to_vec(&t);
        let (d, _) = decode_frame(&bytes).unwrap();
        if let RenderCommand::Text { text, .. } = &d.commands[0] {
            assert_eq!(text, original);
        } else {
            panic!("expected Text command");
        }
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn frame_size_is_compact_for_typical_ui() {
        // A toolbar-like frame: ~30 rects + ~10 text runs ~~ a few KB.
        let mut t = RenderTree::new();
        for i in 0..30 {
            t.commands.push(RenderCommand::FillRect {
                x: i as f32,
                y: 0.0,
                width: 10.0,
                height: 20.0,
                color: Color::rgb(50, 50, 50),
                corner_radii: CornerRadii::ZERO,
            });
        }
        for i in 0..10 {
            t.commands.push(RenderCommand::Text {
                x: i as f32 * 30.0,
                y: 5.0,
                text: "menu item".to_string(),
                color: Color::rgb(200, 200, 200),
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
        let bytes = encode_frame_to_vec(&t);
        // Sanity: comfortably under 4 KB.
        assert!(bytes.len() < 4096, "frame is {} bytes", bytes.len());
        let (d, _) = decode_frame(&bytes).unwrap();
        assert_eq!(d.commands.len(), 40);
    }
}
