//! Multi-window scene protocol — the compositor-level layer of the SlateOS
//! remote-desktop protocol.
//!
//! [`crate::encode_frame`] serialises a single window's draw commands. A real
//! desktop has many windows stacked in z-order, each redrawing independently;
//! this module wraps the per-window command codec into a *scene* frame that
//! carries the whole visible window set plus the metadata a remote viewer needs
//! to composite it: per-window geometry, opacity, stacking order, and the ids
//! of windows that disappeared since the previous frame.
//!
//! The per-window command body is encoded with the very same
//! [`crate::encode_frame`]/[`crate::decode_frame`] used for single-window
//! streaming — there is exactly one draw-command wire codec in this crate.
//!
//! ## Wire format
//!
//! ```text
//! magic    : [u8;4] = b"SCEN"
//! version  : u8     = SCENE_VERSION
//! flags    : u8     = 0 (reserved)
//! sequence : u64                       monotonically increasing frame number
//! disp_w   : u32                       viewer surface width
//! disp_h   : u32                       viewer surface height
//! n_remove : u32                       removed-window count
//!   [u64 ; n_remove]                   ids gone since the previous frame
//! n_win    : u32                       window count, bottom→top z-order
//!   per window:
//!     id      : u64
//!     x, y    : i32, i32               top-left (incl. decorations), LE
//!     w, h    : u32, u32               client size
//!     opacity : f32 (bits)             0.0..=1.0
//!     present : u8                     1 = a command frame follows, 0 = delta
//!     if present: <one inline ORDR frame — see [`crate::encode_frame`]>
//!     n_image : u32                    picture changes for this window
//!       per change:
//!         kind : u8                    1 whole picture, 2 patch, 3 drop
//!         id   : u64                   the window's own id for the picture
//!         whole: w u32, h u32, then w*h pixels
//!         patch: x u32, y u32, w u32, h u32, then w*h pixels
//!         drop : nothing
//!       a pixel is a u32 0xAARRGGBB, little-endian: what the compositor
//!       holds after normalising an upload
//!     video   : u8                     0 nothing new, 1 a frame, 2 stopped
//!     if 1: codec u8 (1 = VP9), w u32, h u32, len u32, then len bytes:
//!           one compressed frame of the window's video
//! ```
//!
//! ## Video
//!
//! A window that presents its own pixels -- a buffer it renders into, as a
//! game or a video player does -- has no commands to forward. Its session
//! codes those pixels as video instead (design.txt's "video-encoded capture"
//! fallback; `design-decisions/1343`): each frame carries at most one
//! compressed frame per window ([`SceneVideo`]), in order, and a viewer
//! decodes every one -- each frame of a VP9 stream is coded against the ones
//! before it -- and shows the latest as the window's content. When the
//! window goes back to commands, the frame says so ([`VideoUpdate::Stop`]).
//! The codec is the sender's and the viewer's business; this crate only
//! carries the bytes.
//!
//! ## Pictures
//!
//! A window draws a picture with a command that names an image *id*; the
//! pixels reached the compositor separately (an upload, and patches to it),
//! and a viewer replaying the commands needs them too. So each window carries
//! the changes to its pictures since its viewer last saw them
//! ([`SceneImage`]): a picture new to the viewer, or too far behind, comes
//! whole; one a few patches behind comes as those patches' rectangles, filled
//! with the picture's pixels *now* -- which is what the viewer's copy should
//! hold there, overlapping patches included, so no patch's own bytes need be
//! kept; one gone comes as a drop. A window leaving the stream takes its
//! pictures with it. [`SceneViewer`] is the viewer's side.
//!
//! ## Delta suppression
//!
//! A [`SceneSession`] fingerprints each window's encoded command frame. When a
//! window's commands are byte-identical to what the viewer already holds, the
//! window is emitted as `present = 0` (geometry only). [`apply_scene_frame`] on
//! the viewer side carries the prior commands forward for such windows, so a
//! static desktop streams as little more than its window rectangles.

use std::collections::BTreeMap;

use guitk::render::RenderTree;

use crate::{DecodeError, Reader, capacity_hint};

/// Scene-frame magic: `b"SCEN"`.
pub const SCENE_MAGIC: [u8; 4] = *b"SCEN";

/// Scene protocol version. Bump on any incompatible layout change.
///
/// **2** -- every window carries the changes to its pictures after its
/// commands (`n_image` and what follows; see the module docs), so a viewer
/// holds the pixels an image command names. Version 1 forwarded none, and a
/// viewer drew nothing wherever a window showed a picture.
///
/// **3** -- and then its video (`video`): a window presenting its own pixels
/// streams them coded, where version 2 sent such a window as an empty
/// command list.
pub const SCENE_VERSION: u8 = 3;

/// [`SceneVideo::codec`] for VP9 (profile 0: 8-bit 4:2:0), the only codec
/// so far.
pub const VIDEO_VP9: u8 = 1;

/// Upper bound on one compressed video frame, to reject corrupt or hostile
/// input before allocating: a VP9 frame of a 4K picture at a generous
/// bitrate is well under a megabyte, a key frame of one at the finest
/// quantiser a few.
pub const MAX_VIDEO_FRAME_BYTES: u32 = 16 << 20;

/// Upper bound on the window count and removed-id count in a single scene
/// frame, to reject corrupt/hostile input before allocating.
pub const MAX_WINDOWS_PER_FRAME: u32 = 1 << 16;

/// Upper bound on one window's picture changes in one frame, for the same
/// reason. A window's pictures are what its program uploaded, and a program
/// with thousands of them sends most of them once.
pub const MAX_IMAGE_CHANGES_PER_WINDOW: u32 = 1 << 12;

/// Scene-frame header: magic + version + flags + sequence + dims + n_remove.
const SCENE_HEADER_LEN: usize = 4 + 1 + 1 + 8 + 4 + 4 + 4;

/// One window's contribution to a scene frame.
#[derive(Clone, Debug)]
pub struct SceneWindow {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    /// `Some` when the window's commands changed (or it is new to the session)
    /// and are forwarded in full; `None` is a delta meaning "reuse the commands
    /// you already have for this window".
    pub commands: Option<RenderTree>,
    /// The changes to the window's pictures since the viewer last saw them,
    /// in the order to apply them.
    pub images: Vec<SceneImage>,
    /// What changed in the window's video, if anything: see [`VideoUpdate`].
    pub video: Option<VideoUpdate>,
}

/// One compressed frame of a window's video.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneVideo {
    /// Which codec: [`VIDEO_VP9`].
    pub codec: u8,
    /// The picture's size in pixels. A stream's frames keep one size; a
    /// window whose pixels change size starts its stream again, with a key
    /// frame.
    pub width: u32,
    pub height: u32,
    /// The compressed frame, at most [`MAX_VIDEO_FRAME_BYTES`].
    pub frame: Vec<u8>,
}

/// What changed in a window's video this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoUpdate {
    /// The stream's next frame, to be decoded after every one before it.
    Frame(SceneVideo),
    /// The window draws commands again: its video is over, and its stream
    /// with it -- video that resumes starts a new one.
    Stop,
}

/// A change to one of a window's pictures, as a viewer applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneImage {
    /// The whole picture, stored under `id` in place of any held there.
    Whole {
        id: u64,
        width: u32,
        height: u32,
        /// `width * height` pixels, row-major, `0xAARRGGBB`.
        pixels: Vec<u32>,
    },
    /// A rectangle of a picture the viewer holds, written over it.
    Patch {
        id: u64,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        /// `width * height` pixels, row-major, `0xAARRGGBB`.
        pixels: Vec<u32>,
    },
    /// The picture is gone; the viewer forgets it.
    Drop { id: u64 },
}

/// A rectangle a patch wrote into a picture, and the revision the picture had
/// after it: one entry of an [`ImageSnapshot`]'s patch log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchMark {
    pub revision: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One of a window's pictures as its owner holds it, for
/// [`SceneSession::build_frame`].
#[derive(Clone, Copy, Debug)]
pub struct ImageSnapshot<'a> {
    /// The window's own id for the picture.
    pub id: u64,
    /// Changed by every upload and every patch, and never repeated: a viewer
    /// holding this revision holds these pixels.
    pub revision: u64,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, row-major, `0xAARRGGBB`.
    pub pixels: &'a [u32],
    /// The oldest revision `patches` can bring up to date. A viewer at this
    /// revision or a later one is sent the rectangles patched since its own; a
    /// viewer at an earlier one -- or holding the picture from before it was
    /// uploaded again -- is sent the whole picture.
    pub patch_base: u64,
    /// The rectangles patched since `patch_base`, oldest first.
    pub patches: &'a [PatchMark],
}

/// A full streamed frame: the visible window set in bottom→top z-order plus the
/// ids that disappeared since the previous frame.
#[derive(Clone, Debug)]
pub struct SceneFrame {
    pub sequence: u64,
    pub display_width: u32,
    pub display_height: u32,
    /// Bottom-to-top z-order (last entry is topmost).
    pub windows: Vec<SceneWindow>,
    /// Window ids present last frame but gone now — the viewer drops them.
    pub removed: Vec<u64>,
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encode a scene frame to its wire representation.
#[must_use]
pub fn encode_scene_frame(frame: &SceneFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint(SCENE_HEADER_LEN, frame.windows.len(), 48));
    out.extend_from_slice(&SCENE_MAGIC);
    out.push(SCENE_VERSION);
    out.push(0); // flags
    crate::write_u64(&mut out, frame.sequence);
    crate::write_u32(&mut out, frame.display_width);
    crate::write_u32(&mut out, frame.display_height);

    // Counts saturate to u32::MAX on overflow rather than silently truncating;
    // the decode side rejects any count above MAX_WINDOWS_PER_FRAME anyway.
    crate::write_u32(
        &mut out,
        u32::try_from(frame.removed.len()).unwrap_or(u32::MAX),
    );
    for &id in &frame.removed {
        crate::write_u64(&mut out, id);
    }

    crate::write_u32(
        &mut out,
        u32::try_from(frame.windows.len()).unwrap_or(u32::MAX),
    );
    for win in &frame.windows {
        crate::write_u64(&mut out, win.id);
        // Window coordinates are signed; encode the raw two's-complement bits.
        crate::write_u32(&mut out, win.x.cast_unsigned());
        crate::write_u32(&mut out, win.y.cast_unsigned());
        crate::write_u32(&mut out, win.width);
        crate::write_u32(&mut out, win.height);
        crate::write_f32(&mut out, win.opacity);
        match &win.commands {
            Some(tree) => {
                out.push(1);
                // Reuse the single-window command codec verbatim.
                crate::encode_frame(tree, &mut out);
            }
            None => out.push(0),
        }
        crate::write_u32(
            &mut out,
            u32::try_from(win.images.len()).unwrap_or(u32::MAX),
        );
        for change in &win.images {
            encode_image(change, &mut out);
        }
        encode_video(win.video.as_ref(), &mut out);
    }
    out
}

const VIDEO_NONE: u8 = 0;
const VIDEO_FRAME: u8 = 1;
const VIDEO_STOP: u8 = 2;

fn encode_video(update: Option<&VideoUpdate>, out: &mut Vec<u8>) {
    match update {
        None => out.push(VIDEO_NONE),
        Some(VideoUpdate::Stop) => out.push(VIDEO_STOP),
        Some(VideoUpdate::Frame(v)) => {
            out.push(VIDEO_FRAME);
            out.push(v.codec);
            crate::write_u32(out, v.width);
            crate::write_u32(out, v.height);
            // A frame past the limit is refused on the way in; the length
            // saturates rather than wraps so that it is refused, not misread.
            crate::write_u32(out, u32::try_from(v.frame.len()).unwrap_or(u32::MAX));
            out.extend_from_slice(&v.frame);
        }
    }
}

fn decode_video(r: &mut Reader<'_>) -> Result<Option<VideoUpdate>, DecodeError> {
    Ok(match r.read_u8()? {
        VIDEO_NONE => None,
        VIDEO_STOP => Some(VideoUpdate::Stop),
        VIDEO_FRAME => {
            let codec = r.read_u8()?;
            if codec != VIDEO_VP9 {
                return Err(DecodeError::BadVideoCodec(codec));
            }
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            let len = r.read_u32()?;
            if len > MAX_VIDEO_FRAME_BYTES {
                return Err(DecodeError::VideoTooLarge(len));
            }
            let len = usize::try_from(len).map_err(|_| DecodeError::VideoTooLarge(len))?;
            Some(VideoUpdate::Frame(SceneVideo {
                codec,
                width,
                height,
                frame: r.take(len)?.to_vec(),
            }))
        }
        other => return Err(DecodeError::BadTag(other)),
    })
}

const IMAGE_WHOLE: u8 = 1;
const IMAGE_PATCH: u8 = 2;
const IMAGE_DROP: u8 = 3;

fn encode_image(change: &SceneImage, out: &mut Vec<u8>) {
    let pixels = |out: &mut Vec<u8>, pixels: &[u32]| {
        out.reserve(pixels.len().saturating_mul(4));
        for &px in pixels {
            crate::write_u32(out, px);
        }
    };
    match change {
        SceneImage::Whole {
            id,
            width,
            height,
            pixels: px,
        } => {
            out.push(IMAGE_WHOLE);
            crate::write_u64(out, *id);
            crate::write_u32(out, *width);
            crate::write_u32(out, *height);
            pixels(out, px);
        }
        SceneImage::Patch {
            id,
            x,
            y,
            width,
            height,
            pixels: px,
        } => {
            out.push(IMAGE_PATCH);
            crate::write_u64(out, *id);
            crate::write_u32(out, *x);
            crate::write_u32(out, *y);
            crate::write_u32(out, *width);
            crate::write_u32(out, *height);
            pixels(out, px);
        }
        SceneImage::Drop { id } => {
            out.push(IMAGE_DROP);
            crate::write_u64(out, *id);
        }
    }
}

/// `width * height` pixels off the wire, refused before anything is allocated
/// when they would be more than an upload may carry
/// ([`crate::MAX_IMAGE_BYTES`]) or more than the frame holds.
fn read_pixels(r: &mut Reader<'_>, width: u32, height: u32) -> Result<Vec<u32>, DecodeError> {
    let bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    let too_large = || DecodeError::ImageTooLarge(u32::try_from(bytes).unwrap_or(u32::MAX));
    if bytes > u64::from(crate::MAX_IMAGE_BYTES) {
        return Err(too_large());
    }
    let len = usize::try_from(bytes).map_err(|_| too_large())?;
    let (pixels, _) = r.take(len)?.as_chunks::<4>();
    Ok(pixels.iter().map(|b| u32::from_le_bytes(*b)).collect())
}

fn decode_image(r: &mut Reader<'_>) -> Result<SceneImage, DecodeError> {
    let kind = r.read_u8()?;
    let id = r.read_u64()?;
    Ok(match kind {
        IMAGE_WHOLE => {
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            SceneImage::Whole {
                id,
                width,
                height,
                pixels: read_pixels(r, width, height)?,
            }
        }
        IMAGE_PATCH => {
            let x = r.read_u32()?;
            let y = r.read_u32()?;
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            SceneImage::Patch {
                id,
                x,
                y,
                width,
                height,
                pixels: read_pixels(r, width, height)?,
            }
        }
        IMAGE_DROP => SceneImage::Drop { id },
        other => return Err(DecodeError::BadTag(other)),
    })
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decode a scene frame from its wire representation, returning it and the
/// number of bytes it occupied.
///
/// The byte count is what lets a scene frame share a stream with other frames:
/// without it a caller can decode one and then has no idea where the next
/// begins, so a scene frame could only ever be alone in its buffer. Every other
/// decoder in this crate reports it for the same reason.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `SCEN`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::UnexpectedEof`] for a short buffer, and
/// [`DecodeError::TooManyWindows`] for a count that would allocate absurdly.
pub fn decode_scene_frame(input: &[u8]) -> Result<(SceneFrame, usize), DecodeError> {
    let mut r = Reader::new(input);
    r.need(SCENE_HEADER_LEN)?;
    r.expect_magic(SCENE_MAGIC)?;
    let ver = r.read_u8()?;
    if ver != SCENE_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let sequence = r.read_u64()?;
    let display_width = r.read_u32()?;
    let display_height = r.read_u32()?;

    let n_remove = r.read_u32()?;
    if n_remove > MAX_WINDOWS_PER_FRAME {
        return Err(DecodeError::TooManyWindows(n_remove));
    }
    let mut removed = Vec::with_capacity(n_remove as usize);
    for _ in 0..n_remove {
        removed.push(r.read_u64()?);
    }

    let n_win = r.read_u32()?;
    if n_win > MAX_WINDOWS_PER_FRAME {
        return Err(DecodeError::TooManyWindows(n_win));
    }
    let mut windows = Vec::with_capacity(n_win as usize);
    for _ in 0..n_win {
        let id = r.read_u64()?;
        let x = r.read_u32()?.cast_signed();
        let y = r.read_u32()?.cast_signed();
        let width = r.read_u32()?;
        let height = r.read_u32()?;
        let opacity = r.read_f32()?;
        let present = r.read_u8()?;
        let commands = match present {
            0 => None,
            1 => {
                // Decode one inline ORDR frame from the remaining bytes and
                // advance our cursor by however many it consumed. `advance`
                // re-checks that many bytes are actually there, so a nested
                // decoder reporting a length longer than the buffer is an
                // error here rather than a cursor left past the end.
                let (tree, consumed) = crate::decode_frame(r.rest())?;
                r.advance(consumed)?;
                Some(tree)
            }
            other => return Err(DecodeError::BadTag(other)),
        };
        let n_image = r.read_u32()?;
        if n_image > MAX_IMAGE_CHANGES_PER_WINDOW {
            return Err(DecodeError::TooManyImageChanges(n_image));
        }
        let mut images = Vec::with_capacity(n_image as usize);
        for _ in 0..n_image {
            images.push(decode_image(&mut r)?);
        }
        let video = decode_video(&mut r)?;
        windows.push(SceneWindow {
            id,
            x,
            y,
            width,
            height,
            opacity,
            commands,
            images,
            video,
        });
    }

    Ok((
        SceneFrame {
            sequence,
            display_width,
            display_height,
            windows,
            removed,
        },
        r.position(),
    ))
}

/// Streaming form of [`decode_scene_frame`]: `Ok(None)` when the buffer holds
/// only part of a frame, so a caller reading from a transport can read more
/// rather than treating a short read as corruption.
///
/// # Errors
///
/// As [`decode_scene_frame`], except that a short buffer is `Ok(None)` rather
/// than [`DecodeError::UnexpectedEof`].
pub fn try_decode_scene_frame(input: &[u8]) -> Result<Option<(SceneFrame, usize)>, DecodeError> {
    match decode_scene_frame(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Session — stateful delta tracking
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash. Cheap, allocation-free fingerprint for change detection
/// only (not cryptographic): a collision would merely suppress one window's
/// redraw for one frame, which the next genuine content change corrects.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// A window's current state, supplied to [`SceneSession::build_frame`] in
/// bottom-to-top z-order. Borrows the live command list to avoid a copy when
/// the window is unchanged (the common case).
pub struct WindowSnapshot<'a> {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub commands: &'a RenderTree,
    /// Every picture the window holds, in any order.
    pub images: Vec<ImageSnapshot<'a>>,
    /// What changed in the window's video since this session's last frame:
    /// the sender codes the window's pixels and says when its video stops
    /// (see the module docs). Forwarded as it is.
    pub video: Option<&'a VideoUpdate>,
}

/// Tracks what one remote viewer already holds, so successive frames forward a
/// window's commands only when its fingerprint changes (geometry-only deltas
/// otherwise), and its pictures only as far as the viewer is behind.
#[derive(Clone, Debug, Default)]
pub struct SceneSession {
    next_sequence: u64,
    /// window id → fingerprint of the last forwarded command frame.
    sent: BTreeMap<u64, u64>,
    /// (window id, picture id) → the revision of the picture the viewer holds.
    images: BTreeMap<(u64, u64), u64>,
}

/// The changes that bring a viewer holding revision `held` of `image` (or not
/// holding it at all) up to date, or none if it is.
fn image_changes(image: &ImageSnapshot<'_>, held: Option<u64>) -> Vec<SceneImage> {
    let whole = || {
        vec![SceneImage::Whole {
            id: image.id,
            width: image.width,
            height: image.height,
            pixels: image.pixels.to_vec(),
        }]
    };
    let Some(held) = held else {
        return whole();
    };
    if held == image.revision {
        return Vec::new();
    }
    if held < image.patch_base {
        return whole();
    }
    let behind: Vec<&PatchMark> = image.patches.iter().filter(|p| p.revision > held).collect();
    // Rectangles that together cover as much as the picture cost more than the
    // picture: send it whole.
    let area: u64 = behind
        .iter()
        .map(|p| u64::from(p.width).saturating_mul(u64::from(p.height)))
        .fold(0, u64::saturating_add);
    if behind.is_empty() || area >= u64::from(image.width).saturating_mul(u64::from(image.height)) {
        return whole();
    }
    let mut out = Vec::with_capacity(behind.len());
    for mark in behind {
        match crop(image, mark) {
            Some(pixels) => out.push(SceneImage::Patch {
                id: image.id,
                x: mark.x,
                y: mark.y,
                width: mark.width,
                height: mark.height,
                pixels,
            }),
            // A mark that does not fit the picture cannot be sent as a patch;
            // the whole picture is always right.
            None => return whole(),
        }
    }
    out
}

/// The picture's pixels now, under `mark`'s rectangle; `None` if the
/// rectangle does not lie inside the picture.
fn crop(image: &ImageSnapshot<'_>, mark: &PatchMark) -> Option<Vec<u32>> {
    let right = mark.x.checked_add(mark.width)?;
    let bottom = mark.y.checked_add(mark.height)?;
    if right > image.width || bottom > image.height {
        return None;
    }
    let stride = usize::try_from(image.width).ok()?;
    let (x, w) = (
        usize::try_from(mark.x).ok()?,
        usize::try_from(mark.width).ok()?,
    );
    let mut out = Vec::with_capacity(w.saturating_mul(usize::try_from(mark.height).ok()?));
    for row in mark.y..bottom {
        let start = usize::try_from(row)
            .ok()?
            .checked_mul(stride)?
            .checked_add(x)?;
        out.extend_from_slice(image.pixels.get(start..start.checked_add(w)?)?);
    }
    Some(out)
}

impl SceneSession {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The sequence number the next [`build_frame`](Self::build_frame) stamps.
    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Build the next [`SceneFrame`] from the current visible window set
    /// (bottom-to-top z-order). A window's commands are included only when its
    /// fingerprint differs from what this session last forwarded; otherwise it
    /// is a geometry-only delta. Windows present in a previous frame but absent
    /// now are reported in [`SceneFrame::removed`].
    pub fn build_frame(
        &mut self,
        display_width: u32,
        display_height: u32,
        windows: &[WindowSnapshot<'_>],
    ) -> SceneFrame {
        let mut out_windows = Vec::with_capacity(windows.len());
        let mut still_present: BTreeMap<u64, u64> = BTreeMap::new();

        let mut images_now: BTreeMap<(u64, u64), u64> = BTreeMap::new();
        for snap in windows {
            let blob = crate::encode_frame_to_vec(snap.commands);
            let fp = fnv1a_64(&blob);
            let changed = self.sent.get(&snap.id) != Some(&fp);
            let commands = if changed {
                Some(snap.commands.clone())
            } else {
                None
            };
            still_present.insert(snap.id, fp);

            // The pictures, in id order so that a frame is the same bytes
            // whatever order the owner keeps them in; then the ones the viewer
            // holds that the window no longer does.
            let mut ordered: Vec<&ImageSnapshot<'_>> = snap.images.iter().collect();
            ordered.sort_by_key(|image| image.id);
            let mut images = Vec::new();
            for image in ordered {
                let key = (snap.id, image.id);
                images.extend(image_changes(image, self.images.get(&key).copied()));
                images_now.insert(key, image.revision);
            }
            for &(window, id) in self
                .images
                .range((snap.id, 0)..=(snap.id, u64::MAX))
                .map(|(k, _)| k)
            {
                if !images_now.contains_key(&(window, id)) {
                    images.push(SceneImage::Drop { id });
                }
            }

            out_windows.push(SceneWindow {
                id: snap.id,
                x: snap.x,
                y: snap.y,
                width: snap.width,
                height: snap.height,
                opacity: snap.opacity,
                commands,
                images,
                video: snap.video.cloned(),
            });
        }

        let removed: Vec<u64> = self
            .sent
            .keys()
            .filter(|id| !still_present.contains_key(id))
            .copied()
            .collect();

        self.sent = still_present;
        // A removed window's pictures went with it on the viewer's side, and a
        // window's dropped pictures are gone: what is held now is exactly what
        // this frame leaves.
        self.images = images_now;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        SceneFrame {
            sequence,
            display_width,
            display_height,
            windows: out_windows,
            removed,
        }
    }

    /// Forget all tracked state. The next frame re-sends every window's commands
    /// and pictures in full — use when a viewer (re)connects, or has fallen out
    /// of step ([`SceneViewer::apply`] refused a frame).
    pub fn reset(&mut self) {
        self.sent.clear();
        self.images.clear();
    }
}

/// Why a viewer could not apply a frame: the stream and the viewer disagree
/// about what the viewer holds. The sender resynchronises with
/// [`SceneSession::reset`], after which a frame carries everything whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// A patch to a picture the viewer does not hold.
    UnknownImage { window: u64, id: u64 },
    /// A patch that does not lie inside the picture, or whose pixels do not
    /// fill it; or a whole picture whose pixels are not `width * height`.
    BadImage { window: u64, id: u64 },
    /// The frame lists one window twice.
    WindowTwice { window: u64 },
    /// A video frame for a window already holding
    /// [`MAX_PENDING_VIDEO_FRAMES`] its owner has not taken: frames cannot
    /// be dropped (each is coded against the last), so the stream must
    /// start again.
    VideoBacklog { window: u64 },
}

/// How many compressed video frames a window holds untaken
/// ([`ViewerWindow::take_video`]) before [`SceneViewer::apply`] refuses
/// more: two seconds at thirty a second, far more than a viewer that decodes
/// as it applies ever holds.
pub const MAX_PENDING_VIDEO_FRAMES: usize = 64;

impl core::fmt::Display for SceneError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownImage { window, id } => {
                write!(
                    f,
                    "patch to picture {id} of window {window}, which is not held"
                )
            }
            Self::BadImage { window, id } => write!(
                f,
                "picture {id} of window {window}: pixels that do not fit their rectangle"
            ),
            Self::WindowTwice { window } => write!(f, "window {window} listed twice"),
            Self::VideoBacklog { window } => write!(
                f,
                "window {window} holds {MAX_PENDING_VIDEO_FRAMES} video frames not taken"
            ),
        }
    }
}

impl std::error::Error for SceneError {}

/// One picture as a viewer holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewerImage {
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, row-major, `0xAARRGGBB`.
    pub pixels: Vec<u32>,
}

/// One window as a viewer holds it: its commands, the pictures they name,
/// and its video.
#[derive(Clone, Debug, Default)]
pub struct ViewerWindow {
    pub commands: RenderTree,
    pub images: BTreeMap<u64, ViewerImage>,
    /// The window's video updates not yet taken, oldest first
    /// ([`Self::take_video`]): frames, each to be decoded after the ones
    /// before it, and a [`VideoUpdate::Stop`] where the window went back to
    /// its commands. A stop drops the frames before it, which will never
    /// show, so it is only ever first.
    pub video: Vec<VideoUpdate>,
}

impl ViewerWindow {
    /// The video updates that arrived since the last call, oldest first, for
    /// the viewer to obey in order: the window shows video in place of its
    /// commands from a frame until a stop. Every frame is to be decoded, as
    /// each is coded against the last; a frame after a stop starts a new
    /// stream, with a key frame.
    pub fn take_video(&mut self) -> Vec<VideoUpdate> {
        core::mem::take(&mut self.video)
    }

    /// How many of the untaken updates are frames.
    fn pending_frames(&self) -> usize {
        self.video
            .iter()
            .filter(|u| matches!(u, VideoUpdate::Frame(_)))
            .count()
    }
}

/// What a remote viewer holds: every window the stream has shown it, with its
/// commands and pictures as the frames so far leave them -- the other side of
/// a [`SceneSession`].
#[derive(Clone, Debug, Default)]
pub struct SceneViewer {
    pub windows: BTreeMap<u64, ViewerWindow>,
}

impl SceneViewer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one decoded frame: windows removed, commands carried forward or
    /// replaced (as [`apply_scene_frame`]), picture changes applied in order.
    ///
    /// All or nothing: a frame the viewer cannot apply leaves it as it was.
    ///
    /// # Errors
    ///
    /// [`SceneError`] when the frame patches a picture the viewer does not hold,
    /// or carries pixels that do not fit their rectangle -- a stream this viewer
    /// has fallen out of step with.
    pub fn apply(&mut self, frame: &SceneFrame) -> Result<(), SceneError> {
        // Checked whole before anything changes, against the pictures' sizes
        // alone, so that a refused frame leaves the viewer as it was without
        // copying what it holds in order to put it back.
        let mut seen = std::collections::BTreeSet::new();
        for win in &frame.windows {
            if !seen.insert(win.id) {
                return Err(SceneError::WindowTwice { window: win.id });
            }
            let mut sizes: BTreeMap<u64, (u32, u32)> = self
                .windows
                .get(&win.id)
                .map(|w| {
                    w.images
                        .iter()
                        .map(|(id, image)| (*id, (image.width, image.height)))
                        .collect()
                })
                .unwrap_or_default();
            for change in &win.images {
                check_image(&mut sizes, win.id, change)?;
            }
            if matches!(win.video, Some(VideoUpdate::Frame(_)))
                && self
                    .windows
                    .get(&win.id)
                    .is_some_and(|w| w.pending_frames() >= MAX_PENDING_VIDEO_FRAMES)
            {
                return Err(SceneError::VideoBacklog { window: win.id });
            }
        }
        let mut prev = core::mem::take(&mut self.windows);
        for win in &frame.windows {
            let mut held = prev.remove(&win.id).unwrap_or_default();
            if let Some(tree) = &win.commands {
                held.commands = tree.clone();
            }
            for change in &win.images {
                apply_image(&mut held.images, change);
            }
            match &win.video {
                None => {}
                Some(frame @ VideoUpdate::Frame(_)) => held.video.push(frame.clone()),
                Some(VideoUpdate::Stop) => {
                    // The frames before a stop will never show: only the stop
                    // is left to obey.
                    held.video.clear();
                    held.video.push(VideoUpdate::Stop);
                }
            }
            self.windows.insert(win.id, held);
        }
        Ok(())
    }
}

/// How many pixels a `width` x `height` rectangle holds.
fn pixel_count(width: u32, height: u32) -> usize {
    usize::try_from(u64::from(width).saturating_mul(u64::from(height))).unwrap_or(usize::MAX)
}

/// Whether `change` applies to pictures of these `sizes`, which it then
/// updates as applying it would.
fn check_image(
    sizes: &mut BTreeMap<u64, (u32, u32)>,
    window: u64,
    change: &SceneImage,
) -> Result<(), SceneError> {
    match change {
        SceneImage::Whole {
            id,
            width,
            height,
            pixels,
        } => {
            if pixels.len() != pixel_count(*width, *height) {
                return Err(SceneError::BadImage { window, id: *id });
            }
            sizes.insert(*id, (*width, *height));
        }
        SceneImage::Patch {
            id,
            x,
            y,
            width,
            height,
            pixels,
        } => {
            let &(image_w, image_h) = sizes
                .get(id)
                .ok_or(SceneError::UnknownImage { window, id: *id })?;
            let fits = x.checked_add(*width).is_some_and(|r| r <= image_w)
                && y.checked_add(*height).is_some_and(|b| b <= image_h);
            if !fits || pixels.len() != pixel_count(*width, *height) {
                return Err(SceneError::BadImage { window, id: *id });
            }
        }
        SceneImage::Drop { id } => {
            sizes.remove(id);
        }
    }
    Ok(())
}

/// Apply a change [`check_image`] has passed. Written not to trust that
/// anyway: a patch that does not fit writes nothing rather than panicking.
fn apply_image(images: &mut BTreeMap<u64, ViewerImage>, change: &SceneImage) {
    match change {
        SceneImage::Whole {
            id,
            width,
            height,
            pixels,
        } => {
            images.insert(
                *id,
                ViewerImage {
                    width: *width,
                    height: *height,
                    pixels: pixels.clone(),
                },
            );
        }
        SceneImage::Patch {
            id,
            x,
            y,
            width,
            pixels,
            ..
        } => {
            let Some(image) = images.get_mut(id) else {
                return;
            };
            let (Ok(stride), Ok(w), Ok(x), Ok(y)) = (
                usize::try_from(image.width),
                usize::try_from(*width),
                usize::try_from(*x),
                usize::try_from(*y),
            ) else {
                return;
            };
            if w == 0 {
                return;
            }
            for (row, source) in pixels.chunks_exact(w).enumerate() {
                let start = y
                    .saturating_add(row)
                    .saturating_mul(stride)
                    .saturating_add(x);
                if let Some(target) = image.pixels.get_mut(start..start.saturating_add(w)) {
                    target.copy_from_slice(source);
                }
            }
        }
        SceneImage::Drop { id } => {
            images.remove(id);
        }
    }
}

/// Reconstruct the full per-window command set from a decoded frame, carrying
/// the previous frame's commands forward for delta (`None`) windows. Returned
/// keyed by window id; `removed` ids simply never appear in the result.
#[must_use]
pub fn apply_scene_frame(
    prev: &BTreeMap<u64, RenderTree>,
    frame: &SceneFrame,
) -> BTreeMap<u64, RenderTree> {
    let mut next: BTreeMap<u64, RenderTree> = BTreeMap::new();
    for win in &frame.windows {
        let tree = match &win.commands {
            Some(t) => t.clone(),
            None => prev.get(&win.id).cloned().unwrap_or_default(),
        };
        next.insert(win.id, tree);
    }
    next
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::color::Color;
    use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
    use guitk::style::CornerRadii;

    fn sample_tree() -> RenderTree {
        RenderTree {
            commands: vec![
                RenderCommand::FillRect {
                    x: 1.5,
                    y: 2.5,
                    width: 100.0,
                    height: 50.0,
                    color: Color::rgba(10, 20, 30, 255),
                    corner_radii: CornerRadii::all(4.0),
                },
                RenderCommand::Text {
                    x: 5.0,
                    y: 6.0,
                    text: "héllo 🦀".to_string(),
                    color: Color::rgba(255, 255, 255, 255),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                },
            ],
        }
    }

    fn assert_tree_eq(a: &RenderTree, b: &RenderTree) {
        assert_eq!(a.commands.len(), b.commands.len());
        for (ca, cb) in a.commands.iter().zip(b.commands.iter()) {
            assert_eq!(format!("{ca:?}"), format!("{cb:?}"));
        }
    }

    #[test]
    fn scene_frame_round_trips() {
        let frame = SceneFrame {
            sequence: 42,
            display_width: 1920,
            display_height: 1080,
            windows: vec![
                SceneWindow {
                    id: 1,
                    x: -10,
                    y: 20,
                    width: 640,
                    height: 480,
                    opacity: 0.75,
                    commands: Some(sample_tree()),
                    images: Vec::new(),
                    video: None,
                },
                SceneWindow {
                    id: 2,
                    x: 100,
                    y: 200,
                    width: 300,
                    height: 150,
                    opacity: 1.0,
                    commands: None,
                    images: Vec::new(),
                    video: None,
                },
            ],
            removed: vec![7, 9],
        };
        let bytes = encode_scene_frame(&frame);
        let (back, used) = decode_scene_frame(&bytes).expect("decode");
        assert_eq!(used, bytes.len(), "a frame must account for all its bytes");
        assert_eq!(back.sequence, 42);
        assert_eq!(back.display_width, 1920);
        assert_eq!(back.display_height, 1080);
        assert_eq!(back.removed, vec![7, 9]);
        assert_eq!(back.windows.len(), 2);
        assert_eq!(back.windows[0].id, 1);
        assert_eq!(back.windows[0].x, -10);
        assert_eq!(back.windows[0].y, 20);
        assert!((back.windows[0].opacity - 0.75).abs() < f32::EPSILON);
        assert_tree_eq(back.windows[0].commands.as_ref().unwrap(), &sample_tree());
        assert!(back.windows[1].commands.is_none());
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = encode_scene_frame(&SceneFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[0] ^= 0xFF;
        assert!(matches!(
            decode_scene_frame(&bytes),
            Err(DecodeError::BadMagic)
        ));
    }

    #[test]
    fn truncated_rejected() {
        let bytes = encode_scene_frame(&SceneFrame {
            sequence: 1,
            display_width: 1,
            display_height: 1,
            windows: vec![SceneWindow {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: Some(sample_tree()),
                images: Vec::new(),
                video: None,
            }],
            removed: vec![],
        });
        assert!(decode_scene_frame(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn two_frames_back_to_back_are_read_in_order() {
        // The point of reporting bytes consumed: a scene frame can share a
        // stream instead of having to be alone in its buffer.
        let mut buf = Vec::new();
        for seq in 1..=3u64 {
            buf.extend_from_slice(&encode_scene_frame(&SceneFrame {
                sequence: seq,
                display_width: 800,
                display_height: 600,
                windows: vec![SceneWindow {
                    id: seq,
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                    opacity: 1.0,
                    commands: Some(sample_tree()),
                    images: Vec::new(),
                    video: None,
                }],
                removed: vec![],
            }));
        }
        let mut at = 0usize;
        let mut seen = Vec::new();
        while at < buf.len() {
            let (frame, used) = decode_scene_frame(&buf[at..]).expect("decode");
            seen.push(frame.sequence);
            at += used;
        }
        assert_eq!(seen, vec![1, 2, 3]);
        assert_eq!(at, buf.len(), "no bytes left over");
    }

    #[test]
    fn a_partial_frame_reads_as_incomplete_rather_than_corrupt() {
        let bytes = encode_scene_frame(&SceneFrame {
            sequence: 1,
            display_width: 640,
            display_height: 480,
            windows: vec![SceneWindow {
                id: 1,
                x: -5,
                y: 5,
                width: 100,
                height: 100,
                opacity: 0.5,
                commands: Some(sample_tree()),
                images: Vec::new(),
                video: None,
            }],
            removed: vec![3],
        });
        for n in 0..bytes.len() {
            assert!(
                matches!(try_decode_scene_frame(&bytes[..n]), Ok(None)),
                "a {n}-byte prefix must read as incomplete, not as an error"
            );
        }
        assert!(try_decode_scene_frame(&bytes).expect("decodes").is_some());
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut bytes = encode_scene_frame(&SceneFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[4] = 0xFE; // version byte after the 4-byte magic
        assert!(matches!(
            decode_scene_frame(&bytes),
            Err(DecodeError::UnsupportedVersion(0xFE))
        ));
    }

    #[test]
    fn session_suppresses_unchanged_then_resends_on_change() {
        let mut session = SceneSession::new();
        let tree_a = sample_tree();
        let snaps_a = vec![WindowSnapshot {
            id: 5,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            opacity: 1.0,
            commands: &tree_a,
            images: Vec::new(),
            video: None,
        }];

        let f0 = session.build_frame(800, 600, &snaps_a);
        assert_eq!(f0.sequence, 0);
        assert!(f0.windows[0].commands.is_some());

        let f1 = session.build_frame(800, 600, &snaps_a);
        assert_eq!(f1.sequence, 1);
        assert!(f1.windows[0].commands.is_none());

        // Change the content → resend.
        let tree_b = RenderTree {
            commands: vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                color: Color::rgba(9, 9, 9, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        };
        let snaps_b = vec![WindowSnapshot {
            id: 5,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            opacity: 1.0,
            commands: &tree_b,
            images: Vec::new(),
            video: None,
        }];
        let f2 = session.build_frame(800, 600, &snaps_b);
        assert!(f2.windows[0].commands.is_some());
    }

    #[test]
    fn session_reports_removed_windows() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let two = vec![
            WindowSnapshot {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &tree,
                images: Vec::new(),
                video: None,
            },
            WindowSnapshot {
                id: 2,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &tree,
                images: Vec::new(),
                video: None,
            },
        ];
        session.build_frame(10, 10, &two);

        let one = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
            images: Vec::new(),
            video: None,
        }];
        let f = session.build_frame(10, 10, &one);
        assert_eq!(f.removed, vec![2]);
    }

    #[test]
    fn apply_scene_frame_carries_forward_deltas() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
            images: Vec::new(),
            video: None,
        }];

        let f0 = session.build_frame(10, 10, &snaps);
        let v0 = apply_scene_frame(&BTreeMap::new(), &f0);
        assert_eq!(
            v0.get(&1).map(|t| t.commands.len()),
            Some(tree.commands.len())
        );

        let f1 = session.build_frame(10, 10, &snaps);
        assert!(f1.windows[0].commands.is_none());
        let v1 = apply_scene_frame(&v0, &f1);
        assert_tree_eq(v1.get(&1).unwrap(), &tree);
    }

    #[test]
    fn reset_forces_full_resend() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
            images: Vec::new(),
            video: None,
        }];
        session.build_frame(10, 10, &snaps);
        session.reset();
        let f = session.build_frame(10, 10, &snaps);
        assert!(f.windows[0].commands.is_some());
    }

    /// A frame with one window carrying `images`, encoded.
    fn frame_with(images: Vec<SceneImage>) -> Vec<u8> {
        encode_scene_frame(&SceneFrame {
            sequence: 1,
            display_width: 10,
            display_height: 10,
            windows: vec![SceneWindow {
                id: 4,
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                opacity: 1.0,
                commands: None,
                images,
                video: None,
            }],
            removed: Vec::new(),
        })
    }

    #[test]
    fn every_kind_of_picture_change_round_trips() {
        let images = vec![
            SceneImage::Whole {
                id: 1,
                width: 3,
                height: 2,
                pixels: vec![1, 2, 3, 4, 5, 0xFFFF_FFFF],
            },
            SceneImage::Patch {
                id: 1,
                x: 1,
                y: 1,
                width: 2,
                height: 1,
                pixels: vec![7, 8],
            },
            SceneImage::Drop { id: 9 },
            SceneImage::Whole {
                id: 2,
                width: 0,
                height: 5,
                pixels: Vec::new(),
            },
        ];
        let bytes = frame_with(images.clone());
        let (frame, used) = decode_scene_frame(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(frame.windows[0].images, images);
    }

    /// Sizes and counts a hostile frame could claim are refused before any
    /// pixel is allocated, and an unknown kind of change by its byte.
    #[test]
    fn a_picture_too_large_or_too_many_or_unknown_is_refused() {
        // A 1x1 picture, its size then rewritten to 100000 x 100000: 40 GB
        // of pixels, refused on the size alone.
        let mut bytes = frame_with(vec![SceneImage::Whole {
            id: 1,
            width: 1,
            height: 1,
            pixels: vec![0],
        }]);
        // Width and height, before the one pixel and the window's video byte.
        let at = bytes.len() - 1 - 4 - 8;
        bytes[at..at + 4].copy_from_slice(&100_000u32.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&100_000u32.to_le_bytes());
        assert!(matches!(
            decode_scene_frame(&bytes),
            Err(DecodeError::ImageTooLarge(_))
        ));

        // More changes than a window may carry, refused on the count.
        let mut bytes = frame_with(Vec::new());
        let at = bytes.len() - 1 - 4;
        bytes[at..at + 4].copy_from_slice(&(MAX_IMAGE_CHANGES_PER_WINDOW + 1).to_le_bytes());
        assert_eq!(
            decode_scene_frame(&bytes).err(),
            Some(DecodeError::TooManyImageChanges(
                MAX_IMAGE_CHANGES_PER_WINDOW + 1
            ))
        );

        // A kind of change that does not exist.
        let mut bytes = frame_with(vec![SceneImage::Drop { id: 3 }]);
        let at = bytes.len() - 1 - 8 - 1;
        bytes[at] = 9;
        assert_eq!(
            decode_scene_frame(&bytes).err(),
            Some(DecodeError::BadTag(9))
        );
    }

    /// A frame with one window carrying `video`, encoded.
    fn frame_with_video(video: Option<VideoUpdate>) -> Vec<u8> {
        encode_scene_frame(&SceneFrame {
            sequence: 1,
            display_width: 10,
            display_height: 10,
            windows: vec![SceneWindow {
                id: 4,
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                opacity: 1.0,
                commands: None,
                images: Vec::new(),
                video,
            }],
            removed: Vec::new(),
        })
    }

    fn vp9_frame(bytes: &[u8]) -> SceneVideo {
        SceneVideo {
            codec: VIDEO_VP9,
            width: 64,
            height: 48,
            frame: bytes.to_vec(),
        }
    }

    /// Nothing, a frame and a stop all come back as they went.
    #[test]
    fn every_kind_of_video_update_round_trips() {
        for video in [
            None,
            Some(VideoUpdate::Frame(vp9_frame(&[0x82, 0x49, 0x83, 1, 2, 3]))),
            Some(VideoUpdate::Frame(vp9_frame(&[]))),
            Some(VideoUpdate::Stop),
        ] {
            let bytes = frame_with_video(video.clone());
            let (frame, used) = decode_scene_frame(&bytes).unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(frame.windows[0].video, video);
        }
    }

    /// A video frame longer than the limit is refused on its length, before
    /// anything is allocated; an unknown codec or kind of update by its byte;
    /// a frame cut short as such.
    #[test]
    fn a_video_frame_too_large_or_unknown_or_short_is_refused() {
        let frame = vp9_frame(&[1, 2, 3, 4]);
        let bytes = frame_with_video(Some(VideoUpdate::Frame(frame)));
        // The length, before the four bytes.
        let len_at = bytes.len() - 4 - 4;
        let mut long = bytes.clone();
        long[len_at..len_at + 4].copy_from_slice(&(MAX_VIDEO_FRAME_BYTES + 1).to_le_bytes());
        assert_eq!(
            decode_scene_frame(&long).err(),
            Some(DecodeError::VideoTooLarge(MAX_VIDEO_FRAME_BYTES + 1))
        );
        // The codec, before the width, height and length.
        let mut codec = bytes.clone();
        codec[len_at - 8 - 1] = 7;
        assert_eq!(
            decode_scene_frame(&codec).err(),
            Some(DecodeError::BadVideoCodec(7))
        );
        // The kind of update, before the codec.
        let mut kind = bytes.clone();
        kind[len_at - 8 - 2] = 5;
        assert_eq!(
            decode_scene_frame(&kind).err(),
            Some(DecodeError::BadTag(5))
        );
        // The frame's bytes cut short.
        assert_eq!(
            decode_scene_frame(&bytes[..bytes.len() - 1]).err(),
            Some(DecodeError::UnexpectedEof)
        );
    }

    /// The session forwards each window's video update as given.
    #[test]
    fn the_session_forwards_video_updates() {
        let tree = RenderTree::new();
        let update = VideoUpdate::Frame(vp9_frame(&[9, 9]));
        let snaps = vec![WindowSnapshot {
            id: 3,
            x: 0,
            y: 0,
            width: 64,
            height: 48,
            opacity: 1.0,
            commands: &tree,
            images: Vec::new(),
            video: Some(&update),
        }];
        let mut session = SceneSession::new();
        let frame = session.build_frame(100, 100, &snaps);
        assert_eq!(frame.windows[0].video.as_ref(), Some(&update));
        let quiet = vec![WindowSnapshot {
            id: 3,
            x: 0,
            y: 0,
            width: 64,
            height: 48,
            opacity: 1.0,
            commands: &tree,
            images: Vec::new(),
            video: None,
        }];
        let frame = session.build_frame(100, 100, &quiet);
        assert_eq!(frame.windows[0].video, None);
    }

    /// A viewer queues a window's video frames in order until they are
    /// taken; a stop forgets the untaken ones; a window holding the most it
    /// may refuses another frame, and the frame's other changes with it.
    #[test]
    fn a_viewer_queues_video_frames_until_taken() {
        let window = |video: Option<VideoUpdate>, commands: Option<RenderTree>| SceneFrame {
            sequence: 0,
            display_width: 10,
            display_height: 10,
            windows: vec![SceneWindow {
                id: 4,
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                opacity: 1.0,
                commands,
                images: Vec::new(),
                video,
            }],
            removed: Vec::new(),
        };
        let mut viewer = SceneViewer::new();
        for n in 0..3u8 {
            let f = window(Some(VideoUpdate::Frame(vp9_frame(&[n]))), None);
            viewer.apply(&f).unwrap();
        }
        viewer.apply(&window(None, None)).unwrap();
        let held = viewer.windows.get_mut(&4).unwrap();
        let first_bytes = |updates: Vec<VideoUpdate>| -> Vec<Option<u8>> {
            updates
                .iter()
                .map(|u| match u {
                    VideoUpdate::Frame(v) => Some(v.frame[0]),
                    VideoUpdate::Stop => None,
                })
                .collect()
        };
        assert_eq!(
            first_bytes(held.take_video()),
            [Some(0), Some(1), Some(2)],
            "every frame, in order"
        );
        assert!(held.take_video().is_empty());

        // A stop drops the frames not taken, which will never show; a frame
        // after it is a new stream's.
        for update in [
            VideoUpdate::Frame(vp9_frame(&[7])),
            VideoUpdate::Stop,
            VideoUpdate::Frame(vp9_frame(&[8])),
        ] {
            viewer.apply(&window(Some(update), None)).unwrap();
        }
        let held = viewer.windows.get_mut(&4).unwrap();
        assert_eq!(first_bytes(held.take_video()), [None, Some(8)]);

        // The stop counts towards no backlog, frames do.
        viewer
            .apply(&window(Some(VideoUpdate::Stop), None))
            .unwrap();
        for n in 0..MAX_PENDING_VIDEO_FRAMES {
            let f = window(Some(VideoUpdate::Frame(vp9_frame(&[n as u8]))), None);
            viewer.apply(&f).unwrap();
        }
        let one_more = window(
            Some(VideoUpdate::Frame(vp9_frame(&[0]))),
            Some(sample_tree()),
        );
        assert_eq!(
            viewer.apply(&one_more),
            Err(SceneError::VideoBacklog { window: 4 })
        );
        let held = &viewer.windows[&4];
        assert_eq!(held.video.len(), MAX_PENDING_VIDEO_FRAMES + 1);
        assert_eq!(held.video.first(), Some(&VideoUpdate::Stop));
        assert!(
            held.commands.commands.is_empty(),
            "the refused frame changed nothing"
        );
    }

    /// The session sends a picture whole the first time, nothing while it is
    /// unchanged, the patched rectangles after a patch, the whole picture again
    /// once the viewer is further behind than the log reaches, and a drop once
    /// the window no longer holds it.
    #[test]
    fn the_session_sends_each_picture_only_as_far_as_its_viewer_is_behind() {
        fn snap<'a>(
            tree: &'a RenderTree,
            revision: u64,
            pixels: &'a [u32],
            base: u64,
            patches: &'a [PatchMark],
        ) -> Vec<WindowSnapshot<'a>> {
            let image = ImageSnapshot {
                id: 5,
                revision,
                width: 4,
                height: 4,
                pixels,
                patch_base: base,
                patches,
            };
            vec![WindowSnapshot {
                id: 1,
                x: 0,
                y: 0,
                width: 4,
                height: 4,
                opacity: 1.0,
                commands: tree,
                images: vec![image],
                video: None,
            }]
        }
        let tree = sample_tree();
        let mut pixels = vec![0u32; 16];
        let mut session = SceneSession::new();
        let first = session.build_frame(4, 4, &snap(&tree, 10, &pixels, 10, &[]));
        assert!(matches!(
            first.windows[0].images.as_slice(),
            [SceneImage::Whole { id: 5, .. }]
        ));
        let again = session.build_frame(4, 4, &snap(&tree, 10, &pixels, 10, &[]));
        assert!(again.windows[0].images.is_empty());

        pixels[5] = 0xAB;
        let mark = PatchMark {
            revision: 11,
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };
        let patched = session.build_frame(4, 4, &snap(&tree, 11, &pixels, 10, &[mark]));
        assert_eq!(
            patched.windows[0].images,
            [SceneImage::Patch {
                id: 5,
                x: 1,
                y: 1,
                width: 1,
                height: 1,
                pixels: vec![0xAB],
            }]
        );

        // Two patches later, with the log moved on past revision 11.
        let later = PatchMark {
            revision: 13,
            ..mark
        };
        let behind = session.build_frame(4, 4, &snap(&tree, 13, &pixels, 12, &[later]));
        assert!(matches!(
            behind.windows[0].images.as_slice(),
            [SceneImage::Whole { id: 5, .. }]
        ));

        let mut gone = snap(&tree, 13, &pixels, 12, &[]);
        gone[0].images.clear();
        let dropped = session.build_frame(4, 4, &gone);
        assert_eq!(dropped.windows[0].images, [SceneImage::Drop { id: 5 }]);
    }
}
