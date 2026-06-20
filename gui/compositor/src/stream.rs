//! Native compositor-level streaming — efficient draw-command forwarding.
//!
//! Instead of capturing the composited framebuffer as pixels and shipping a
//! large raster every frame, this module forwards the *vector* draw commands
//! (`RenderCommand`s) that each window submits. A remote viewer reconstructs
//! the scene by replaying those commands through its own rasterizer. For the
//! typical desktop — flat-shaded rectangles, text, borders — a frame of draw
//! commands is orders of magnitude smaller than a 4K raster, so this is the
//! efficient transport for remote desktop / screen sharing.
//!
//! ## Wire format
//!
//! A frame is a flat little-endian byte stream:
//!
//! ```text
//! magic   : u32  = 0x53_4C_54_53  ("SLTS")
//! version : u8   = STREAM_VERSION
//! sequence: u64                       monotonically increasing frame number
//! disp_w  : u32                       display width  (viewer surface size)
//! disp_h  : u32                       display height
//! n_remove: u32                       count of removed window ids
//!   [u64 ; n_remove]                  ids gone since the previous frame
//! n_win   : u32                       count of window records (bottom→top z)
//!   per window:
//!     id      : u64
//!     x,y     : i32, i32              top-left (incl. decorations)
//!     w,h     : u32, u32              client size
//!     opacity : f32 (bits)            0.0..=1.0
//!     present : u8                    1 = commands follow, 0 = unchanged (delta)
//!     if present:
//!        n_cmd : u32
//!          [command ; n_cmd]
//! ```
//!
//! ## Delta suppression
//!
//! A [`StreamSession`] remembers a per-window fingerprint of the last command
//! list it forwarded. When a window's commands are byte-identical to what the
//! viewer already has, the frame carries `present = 0` and omits the command
//! body — only the (cheap) geometry/opacity/z-order is sent. This makes a
//! static desktop nearly free to stream while still allowing per-window
//! incremental redraws.

use std::collections::BTreeMap;

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

/// Magic identifying a SlateOS draw-command stream frame ("SLTS").
pub const STREAM_MAGIC: u32 = 0x53_4C_54_53;

/// Wire-format version. Bump on any incompatible layout change.
pub const STREAM_VERSION: u8 = 1;

/// Errors produced while decoding a stream frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// Ran out of bytes mid-field.
    Truncated,
    /// First four bytes were not [`STREAM_MAGIC`].
    BadMagic(u32),
    /// Frame version is not understood by this decoder.
    UnsupportedVersion(u8),
    /// Encountered an unknown command/enum tag byte.
    UnknownTag(u8),
    /// A length/count field exceeded a sane bound (anti-DoS).
    TooLarge(u32),
}

impl core::fmt::Display for StreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StreamError::Truncated => write!(f, "stream truncated"),
            StreamError::BadMagic(m) => write!(f, "bad stream magic: {m:#010x}"),
            StreamError::UnsupportedVersion(v) => write!(f, "unsupported stream version: {v}"),
            StreamError::UnknownTag(t) => write!(f, "unknown stream tag: {t}"),
            StreamError::TooLarge(n) => write!(f, "stream count too large: {n}"),
        }
    }
}

impl std::error::Error for StreamError {}

/// Upper bound on any count field, to reject corrupt/malicious frames before
/// allocating. No real frame has more windows or commands than this.
const MAX_COUNT: u32 = 1_000_000;

/// One window's contribution to a streamed frame.
#[derive(Clone, Debug)]
pub struct StreamWindow {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    /// `Some` when the command list changed (or the window is new to the
    /// session) and is being forwarded; `None` is a delta meaning "reuse the
    /// commands you already have for this window".
    pub commands: Option<Vec<RenderCommand>>,
}

/// A single forwarded frame: the full visible window set in z-order plus the
/// ids that disappeared since the previous frame.
#[derive(Clone, Debug)]
pub struct StreamFrame {
    pub sequence: u64,
    pub display_width: u32,
    pub display_height: u32,
    /// Bottom-to-top z-order (last entry is topmost), matching the compositor's
    /// `z_stack` so the viewer composites in the same order.
    pub windows: Vec<StreamWindow>,
    /// Window ids present last frame but gone now — the viewer drops them.
    pub removed: Vec<u64>,
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Little-endian byte writer. Allocation-amortizing append-only buffer.
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn color(&mut self, c: Color) {
        self.buf.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    fn radii(&mut self, r: CornerRadii) {
        self.f32(r.top_left);
        self.f32(r.top_right);
        self.f32(r.bottom_right);
        self.f32(r.bottom_left);
    }
    fn str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        // Length-prefixed UTF-8. Strings here are window text, always small.
        self.u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }
    fn opt_f32(&mut self, v: Option<f32>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.f32(x);
            }
            None => self.u8(0),
        }
    }
}

// RenderCommand tag bytes (stable wire identifiers).
const TAG_FILL_RECT: u8 = 1;
const TAG_STROKE_RECT: u8 = 2;
const TAG_TEXT: u8 = 3;
const TAG_IMAGE: u8 = 4;
const TAG_LINE: u8 = 5;
const TAG_PUSH_CLIP: u8 = 6;
const TAG_POP_CLIP: u8 = 7;
const TAG_PUSH_TRANSLATE: u8 = 8;
const TAG_POP_TRANSLATE: u8 = 9;
const TAG_BOX_SHADOW: u8 = 10;

fn encode_weight(w: FontWeightHint) -> u8 {
    match w {
        FontWeightHint::Regular => 0,
        FontWeightHint::Bold => 1,
        FontWeightHint::Light => 2,
    }
}

fn decode_weight(b: u8) -> Result<FontWeightHint, StreamError> {
    match b {
        0 => Ok(FontWeightHint::Regular),
        1 => Ok(FontWeightHint::Bold),
        2 => Ok(FontWeightHint::Light),
        other => Err(StreamError::UnknownTag(other)),
    }
}

fn encode_command(w: &mut Writer, cmd: &RenderCommand) {
    match cmd {
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii,
        } => {
            w.u8(TAG_FILL_RECT);
            w.f32(*x);
            w.f32(*y);
            w.f32(*width);
            w.f32(*height);
            w.color(*color);
            w.radii(*corner_radii);
        }
        RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color,
            line_width,
            corner_radii,
        } => {
            w.u8(TAG_STROKE_RECT);
            w.f32(*x);
            w.f32(*y);
            w.f32(*width);
            w.f32(*height);
            w.color(*color);
            w.f32(*line_width);
            w.radii(*corner_radii);
        }
        RenderCommand::Text {
            x,
            y,
            text,
            color,
            font_size,
            font_weight,
            max_width,
        } => {
            w.u8(TAG_TEXT);
            w.f32(*x);
            w.f32(*y);
            w.str(text);
            w.color(*color);
            w.f32(*font_size);
            w.u8(encode_weight(*font_weight));
            w.opt_f32(*max_width);
        }
        RenderCommand::Image {
            x,
            y,
            width,
            height,
            image_id,
        } => {
            w.u8(TAG_IMAGE);
            w.f32(*x);
            w.f32(*y);
            w.f32(*width);
            w.f32(*height);
            w.u64(*image_id);
        }
        RenderCommand::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => {
            w.u8(TAG_LINE);
            w.f32(*x1);
            w.f32(*y1);
            w.f32(*x2);
            w.f32(*y2);
            w.color(*color);
            w.f32(*width);
        }
        RenderCommand::PushClip {
            x,
            y,
            width,
            height,
        } => {
            w.u8(TAG_PUSH_CLIP);
            w.f32(*x);
            w.f32(*y);
            w.f32(*width);
            w.f32(*height);
        }
        RenderCommand::PopClip => w.u8(TAG_POP_CLIP),
        RenderCommand::PushTranslate { dx, dy } => {
            w.u8(TAG_PUSH_TRANSLATE);
            w.f32(*dx);
            w.f32(*dy);
        }
        RenderCommand::PopTranslate => w.u8(TAG_POP_TRANSLATE),
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
            w.u8(TAG_BOX_SHADOW);
            w.f32(*x);
            w.f32(*y);
            w.f32(*width);
            w.f32(*height);
            w.f32(*offset_x);
            w.f32(*offset_y);
            w.f32(*blur);
            w.f32(*spread);
            w.color(*color);
            w.radii(*corner_radii);
        }
    }
}

/// Encode a list of draw commands into a self-contained little-endian blob.
///
/// Used both for the frame body and (by [`StreamSession`]) as the fingerprint
/// source for delta suppression — identical commands produce identical bytes.
#[must_use]
pub fn encode_commands(commands: &[RenderCommand]) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(commands.len() as u32);
    for cmd in commands {
        encode_command(&mut w, cmd);
    }
    w.buf
}

/// Encode a full frame to its wire representation.
#[must_use]
pub fn encode_frame(frame: &StreamFrame) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(STREAM_MAGIC);
    w.u8(STREAM_VERSION);
    w.u64(frame.sequence);
    w.u32(frame.display_width);
    w.u32(frame.display_height);

    w.u32(frame.removed.len() as u32);
    for &id in &frame.removed {
        w.u64(id);
    }

    w.u32(frame.windows.len() as u32);
    for win in &frame.windows {
        w.u64(win.id);
        w.i32(win.x);
        w.i32(win.y);
        w.u32(win.width);
        w.u32(win.height);
        w.f32(win.opacity);
        match &win.commands {
            Some(cmds) => {
                w.u8(1);
                w.u32(cmds.len() as u32);
                for cmd in cmds {
                    encode_command(&mut w, cmd);
                }
            }
            None => w.u8(0),
        }
    }
    w.buf
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Little-endian byte reader with bounds checking on every field.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], StreamError> {
        let end = self.pos.checked_add(n).ok_or(StreamError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(StreamError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, StreamError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, StreamError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, StreamError> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64, StreamError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn f32(&mut self) -> Result<f32, StreamError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn color(&mut self) -> Result<Color, StreamError> {
        let b = self.take(4)?;
        Ok(Color::rgba(b[0], b[1], b[2], b[3]))
    }
    fn radii(&mut self) -> Result<CornerRadii, StreamError> {
        Ok(CornerRadii {
            top_left: self.f32()?,
            top_right: self.f32()?,
            bottom_right: self.f32()?,
            bottom_left: self.f32()?,
        })
    }
    fn count(&mut self) -> Result<u32, StreamError> {
        let n = self.u32()?;
        if n > MAX_COUNT {
            return Err(StreamError::TooLarge(n));
        }
        Ok(n)
    }
    fn str(&mut self) -> Result<String, StreamError> {
        let len = self.count()? as usize;
        let bytes = self.take(len)?;
        // Window text is UTF-8 on the wire; tolerate malformed input losslessly
        // is impossible, so reject rather than silently corrupt.
        String::from_utf8(bytes.to_vec()).map_err(|_| StreamError::Truncated)
    }
    fn opt_f32(&mut self) -> Result<Option<f32>, StreamError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.f32()?)),
            other => Err(StreamError::UnknownTag(other)),
        }
    }
}

fn decode_command(r: &mut Reader<'_>) -> Result<RenderCommand, StreamError> {
    let tag = r.u8()?;
    Ok(match tag {
        TAG_FILL_RECT => RenderCommand::FillRect {
            x: r.f32()?,
            y: r.f32()?,
            width: r.f32()?,
            height: r.f32()?,
            color: r.color()?,
            corner_radii: r.radii()?,
        },
        TAG_STROKE_RECT => RenderCommand::StrokeRect {
            x: r.f32()?,
            y: r.f32()?,
            width: r.f32()?,
            height: r.f32()?,
            color: r.color()?,
            line_width: r.f32()?,
            corner_radii: r.radii()?,
        },
        TAG_TEXT => RenderCommand::Text {
            x: r.f32()?,
            y: r.f32()?,
            text: r.str()?,
            color: r.color()?,
            font_size: r.f32()?,
            font_weight: decode_weight(r.u8()?)?,
            max_width: r.opt_f32()?,
        },
        TAG_IMAGE => RenderCommand::Image {
            x: r.f32()?,
            y: r.f32()?,
            width: r.f32()?,
            height: r.f32()?,
            image_id: r.u64()?,
        },
        TAG_LINE => RenderCommand::Line {
            x1: r.f32()?,
            y1: r.f32()?,
            x2: r.f32()?,
            y2: r.f32()?,
            color: r.color()?,
            width: r.f32()?,
        },
        TAG_PUSH_CLIP => RenderCommand::PushClip {
            x: r.f32()?,
            y: r.f32()?,
            width: r.f32()?,
            height: r.f32()?,
        },
        TAG_POP_CLIP => RenderCommand::PopClip,
        TAG_PUSH_TRANSLATE => RenderCommand::PushTranslate {
            dx: r.f32()?,
            dy: r.f32()?,
        },
        TAG_POP_TRANSLATE => RenderCommand::PopTranslate,
        TAG_BOX_SHADOW => RenderCommand::BoxShadow {
            x: r.f32()?,
            y: r.f32()?,
            width: r.f32()?,
            height: r.f32()?,
            offset_x: r.f32()?,
            offset_y: r.f32()?,
            blur: r.f32()?,
            spread: r.f32()?,
            color: r.color()?,
            corner_radii: r.radii()?,
        },
        other => return Err(StreamError::UnknownTag(other)),
    })
}

fn decode_command_list(r: &mut Reader<'_>) -> Result<Vec<RenderCommand>, StreamError> {
    let n = r.count()? as usize;
    let mut cmds = Vec::with_capacity(n.min(MAX_COUNT as usize));
    for _ in 0..n {
        cmds.push(decode_command(r)?);
    }
    Ok(cmds)
}

/// Decode a list of commands previously produced by [`encode_commands`].
pub fn decode_commands(bytes: &[u8]) -> Result<Vec<RenderCommand>, StreamError> {
    let mut r = Reader::new(bytes);
    decode_command_list(&mut r)
}

/// Decode a full frame from its wire representation.
pub fn decode_frame(bytes: &[u8]) -> Result<StreamFrame, StreamError> {
    let mut r = Reader::new(bytes);
    let magic = r.u32()?;
    if magic != STREAM_MAGIC {
        return Err(StreamError::BadMagic(magic));
    }
    let version = r.u8()?;
    if version != STREAM_VERSION {
        return Err(StreamError::UnsupportedVersion(version));
    }
    let sequence = r.u64()?;
    let display_width = r.u32()?;
    let display_height = r.u32()?;

    let n_remove = r.count()? as usize;
    let mut removed = Vec::with_capacity(n_remove.min(MAX_COUNT as usize));
    for _ in 0..n_remove {
        removed.push(r.u64()?);
    }

    let n_win = r.count()? as usize;
    let mut windows = Vec::with_capacity(n_win.min(MAX_COUNT as usize));
    for _ in 0..n_win {
        let id = r.u64()?;
        let x = r.i32()?;
        let y = r.i32()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let opacity = r.f32()?;
        let present = r.u8()?;
        let commands = match present {
            0 => None,
            1 => Some(decode_command_list(&mut r)?),
            other => return Err(StreamError::UnknownTag(other)),
        };
        windows.push(StreamWindow {
            id,
            x,
            y,
            width,
            height,
            opacity,
            commands,
        });
    }

    Ok(StreamFrame {
        sequence,
        display_width,
        display_height,
        windows,
        removed,
    })
}

// ---------------------------------------------------------------------------
// Session — stateful delta tracking
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash. Cheap, allocation-free fingerprint for delta detection.
/// We only need change-detection (not cryptographic strength); a collision
/// would merely suppress a redraw for one window for one frame, which the next
/// content change corrects.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Tracks what a single remote viewer already knows, so the compositor can emit
/// minimal frames: a window's commands are forwarded only when its content
/// fingerprint changes (or it is newly visible to this viewer).
#[derive(Clone, Debug, Default)]
pub struct StreamSession {
    next_sequence: u64,
    /// window id → fingerprint of the last forwarded command list.
    sent: BTreeMap<u64, u64>,
}

/// A window's current state as seen by [`StreamSession::build_frame`]. The
/// caller (the compositor) supplies these in bottom-to-top z-order.
pub struct WindowSnapshot<'a> {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub commands: &'a [RenderCommand],
}

impl StreamSession {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The sequence number that the next [`build_frame`] will stamp.
    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Build the next [`StreamFrame`] from the current visible window set.
    ///
    /// `windows` must be in bottom-to-top z-order (matching the compositor's
    /// `z_stack`). For each window, the command list is included only when its
    /// fingerprint differs from what this session last forwarded; otherwise the
    /// window is sent as a geometry-only delta. Windows present in a previous
    /// frame but absent now are reported in [`StreamFrame::removed`].
    pub fn build_frame(
        &mut self,
        display_width: u32,
        display_height: u32,
        windows: &[WindowSnapshot<'_>],
    ) -> StreamFrame {
        let mut out_windows = Vec::with_capacity(windows.len());
        let mut still_present: BTreeMap<u64, u64> = BTreeMap::new();

        for snap in windows {
            let blob = encode_commands(snap.commands);
            let fp = fnv1a_64(&blob);
            let changed = self.sent.get(&snap.id) != Some(&fp);
            let commands = if changed {
                Some(snap.commands.to_vec())
            } else {
                None
            };
            still_present.insert(snap.id, fp);
            out_windows.push(StreamWindow {
                id: snap.id,
                x: snap.x,
                y: snap.y,
                width: snap.width,
                height: snap.height,
                opacity: snap.opacity,
                commands,
            });
        }

        // Any id we knew about but that is no longer present has been removed.
        let removed: Vec<u64> = self
            .sent
            .keys()
            .filter(|id| !still_present.contains_key(id))
            .copied()
            .collect();

        self.sent = still_present;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        StreamFrame {
            sequence,
            display_width,
            display_height,
            windows: out_windows,
            removed,
        }
    }

    /// Forget all tracked state. The next frame re-sends every window's
    /// commands in full — use when a viewer (re)connects.
    pub fn reset(&mut self) {
        self.sent.clear();
        // Sequence is intentionally not reset: viewers detect a gap/restart via
        // the full-resend (every window carries commands) rather than seq == 0.
    }
}

/// Replay a decoded frame into a [`RenderTree`] per window, carrying forward the
/// previous frame's commands for delta (`None`) windows. Returned in z-order.
///
/// This is the viewer-side helper: it turns the wire delta back into a complete
/// scene by merging against the previously reconstructed window commands.
pub fn apply_frame(
    prev: &BTreeMap<u64, Vec<RenderCommand>>,
    frame: &StreamFrame,
) -> BTreeMap<u64, Vec<RenderCommand>> {
    let mut next: BTreeMap<u64, Vec<RenderCommand>> = BTreeMap::new();
    for win in &frame.windows {
        let cmds = match &win.commands {
            Some(c) => c.clone(),
            None => prev.get(&win.id).cloned().unwrap_or_default(),
        };
        next.insert(win.id, cmds);
    }
    // `removed` ids simply never make it into `next`.
    next
}

/// Reconstruct a window's `RenderTree` from a command list (viewer convenience).
#[must_use]
pub fn tree_from_commands(commands: &[RenderCommand]) -> RenderTree {
    RenderTree {
        commands: commands.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::color::Color;
    use guitk::style::CornerRadii;

    fn sample_commands() -> Vec<RenderCommand> {
        vec![
            RenderCommand::FillRect {
                x: 1.5,
                y: 2.5,
                width: 100.0,
                height: 50.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::all(4.0),
            },
            RenderCommand::StrokeRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                color: Color::rgba(1, 2, 3, 200),
                line_width: 2.0,
                corner_radii: CornerRadii::ZERO,
            },
            RenderCommand::Text {
                x: 5.0,
                y: 6.0,
                text: "héllo 🦀".to_string(),
                color: Color::rgba(255, 255, 255, 255),
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
            },
            RenderCommand::Text {
                x: 5.0,
                y: 20.0,
                text: "no max".to_string(),
                color: Color::rgba(0, 0, 0, 255),
                font_size: 12.0,
                font_weight: FontWeightHint::Light,
                max_width: None,
            },
            RenderCommand::Image {
                x: 3.0,
                y: 4.0,
                width: 32.0,
                height: 32.0,
                image_id: 0xDEAD_BEEF_CAFE_F00D,
            },
            RenderCommand::Line {
                x1: 0.0,
                y1: 0.0,
                x2: 9.0,
                y2: 9.0,
                color: Color::rgba(7, 8, 9, 10),
                width: 1.0,
            },
            RenderCommand::PushClip {
                x: 1.0,
                y: 1.0,
                width: 8.0,
                height: 8.0,
            },
            RenderCommand::PopClip,
            RenderCommand::PushTranslate { dx: 2.0, dy: -3.0 },
            RenderCommand::PopTranslate,
            RenderCommand::BoxShadow {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                offset_x: 0.5,
                offset_y: 0.5,
                blur: 2.0,
                spread: 1.0,
                color: Color::rgba(0, 0, 0, 128),
                corner_radii: CornerRadii::all(2.0),
            },
        ]
    }

    fn assert_cmd_eq(a: &RenderCommand, b: &RenderCommand) {
        // RenderCommand isn't PartialEq (floats); compare via debug formatting,
        // which is exact for the bit patterns we round-trip.
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn command_list_round_trips_every_variant() {
        let cmds = sample_commands();
        let blob = encode_commands(&cmds);
        let back = decode_commands(&blob).expect("decode");
        assert_eq!(cmds.len(), back.len());
        for (a, b) in cmds.iter().zip(back.iter()) {
            assert_cmd_eq(a, b);
        }
    }

    #[test]
    fn frame_round_trips() {
        let frame = StreamFrame {
            sequence: 42,
            display_width: 1920,
            display_height: 1080,
            windows: vec![
                StreamWindow {
                    id: 1,
                    x: -10,
                    y: 20,
                    width: 640,
                    height: 480,
                    opacity: 0.75,
                    commands: Some(sample_commands()),
                },
                StreamWindow {
                    id: 2,
                    x: 100,
                    y: 200,
                    width: 300,
                    height: 150,
                    opacity: 1.0,
                    commands: None,
                },
            ],
            removed: vec![7, 9],
        };
        let bytes = encode_frame(&frame);
        let back = decode_frame(&bytes).expect("decode frame");
        assert_eq!(back.sequence, frame.sequence);
        assert_eq!(back.display_width, frame.display_width);
        assert_eq!(back.display_height, frame.display_height);
        assert_eq!(back.removed, frame.removed);
        assert_eq!(back.windows.len(), 2);
        assert!(back.windows[1].commands.is_none());
        assert_eq!(back.windows[0].id, 1);
        assert_eq!(back.windows[0].x, -10);
        // command body matches
        let a = frame.windows[0].commands.as_ref().unwrap();
        let b = back.windows[0].commands.as_ref().unwrap();
        for (ca, cb) in a.iter().zip(b.iter()) {
            assert_cmd_eq(ca, cb);
        }
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = encode_frame(&StreamFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[0] ^= 0xFF;
        assert!(matches!(decode_frame(&bytes), Err(StreamError::BadMagic(_))));
    }

    #[test]
    fn truncated_rejected() {
        let bytes = encode_frame(&StreamFrame {
            sequence: 1,
            display_width: 1,
            display_height: 1,
            windows: vec![StreamWindow {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: Some(sample_commands()),
            }],
            removed: vec![],
        });
        // Lop off the tail; decoding must error rather than panic.
        assert!(decode_frame(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut bytes = encode_frame(&StreamFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[4] = 0xFE; // version byte sits right after the 4-byte magic
        assert!(matches!(
            decode_frame(&bytes),
            Err(StreamError::UnsupportedVersion(0xFE))
        ));
    }

    #[test]
    fn oversized_count_rejected() {
        // magic + version + seq + w + h, then a removed-count of u32::MAX.
        let mut w = Writer::new();
        w.u32(STREAM_MAGIC);
        w.u8(STREAM_VERSION);
        w.u64(0);
        w.u32(1);
        w.u32(1);
        w.u32(u32::MAX); // removed count
        assert!(matches!(
            decode_frame(&w.buf),
            Err(StreamError::TooLarge(_))
        ));
    }

    #[test]
    fn session_suppresses_unchanged_windows() {
        let mut session = StreamSession::new();
        let cmds = sample_commands();
        let snaps = vec![WindowSnapshot {
            id: 5,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            opacity: 1.0,
            commands: &cmds,
        }];

        // First frame: new window → commands present.
        let f0 = session.build_frame(800, 600, &snaps);
        assert_eq!(f0.sequence, 0);
        assert!(f0.windows[0].commands.is_some());
        assert!(f0.removed.is_empty());

        // Second frame, identical content → delta (commands omitted).
        let f1 = session.build_frame(800, 600, &snaps);
        assert_eq!(f1.sequence, 1);
        assert!(f1.windows[0].commands.is_none());
    }

    #[test]
    fn session_resends_on_content_change() {
        let mut session = StreamSession::new();
        let cmds_a = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            color: Color::rgba(1, 1, 1, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        let cmds_b = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            color: Color::rgba(2, 2, 2, 255), // different color
            corner_radii: CornerRadii::ZERO,
        }];
        let snap_a = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            opacity: 1.0,
            commands: &cmds_a,
        }];
        let snap_b = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            opacity: 1.0,
            commands: &cmds_b,
        }];

        assert!(session.build_frame(10, 10, &snap_a).windows[0]
            .commands
            .is_some());
        // unchanged
        assert!(session.build_frame(10, 10, &snap_a).windows[0]
            .commands
            .is_none());
        // changed → resend
        assert!(session.build_frame(10, 10, &snap_b).windows[0]
            .commands
            .is_some());
    }

    #[test]
    fn session_reports_removed_windows() {
        let mut session = StreamSession::new();
        let cmds = sample_commands();
        let two = vec![
            WindowSnapshot {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &cmds,
            },
            WindowSnapshot {
                id: 2,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &cmds,
            },
        ];
        session.build_frame(10, 10, &two);

        // Drop window 2.
        let one = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &cmds,
        }];
        let f = session.build_frame(10, 10, &one);
        assert_eq!(f.removed, vec![2]);
    }

    #[test]
    fn apply_frame_carries_forward_deltas() {
        let mut session = StreamSession::new();
        let cmds = sample_commands();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &cmds,
        }];

        // Frame 0 carries the full command list.
        let f0 = session.build_frame(10, 10, &snaps);
        let v0 = apply_frame(&BTreeMap::new(), &f0);
        assert_eq!(v0.get(&1).map(Vec::len), Some(cmds.len()));

        // Frame 1 is a delta (commands None); apply_frame must reuse v0.
        let f1 = session.build_frame(10, 10, &snaps);
        assert!(f1.windows[0].commands.is_none());
        let v1 = apply_frame(&v0, &f1);
        assert_eq!(v1.get(&1).map(Vec::len), Some(cmds.len()));
        // And the carried-forward commands match the originals bit-for-bit.
        for (a, b) in cmds.iter().zip(v1.get(&1).unwrap().iter()) {
            assert_cmd_eq(a, b);
        }
    }

    #[test]
    fn reset_forces_full_resend() {
        let mut session = StreamSession::new();
        let cmds = sample_commands();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &cmds,
        }];
        session.build_frame(10, 10, &snaps);
        // Without reset this would be a delta.
        session.reset();
        let f = session.build_frame(10, 10, &snaps);
        assert!(f.windows[0].commands.is_some(), "reset must re-send full");
    }
}
