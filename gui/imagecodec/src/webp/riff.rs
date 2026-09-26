//! WebP's RIFF container, read as libwebp reads it.
//!
//! A WebP file is a RIFF file of chunks. What a file means -- one picture or
//! an animation, where each frame's bitstream is, whether it has alpha -- and
//! whether it is acceptable at all is decided, in libwebp, by three readers
//! that all must agree, and this module is a port of each:
//!
//! * [`features`] is `WebPGetFeatures` (`src/dec/webp_dec.c`,
//!   `ParseHeadersInternal`): a quick look at the RIFF header, the `VP8X`
//!   chunk, the optional chunks and the bitstream's own header. libwebp's
//!   animation decoder refuses a file this refuses, and Pillow asks it whether
//!   a picture has alpha.
//! * [`Demuxer::read`] is `WebPDemux` (`src/demux/demux.c`): the full walk of
//!   the chunks, with the demuxer's own rules -- which chunks may follow
//!   which, frames inside the canvas, no flags that do not exist -- building
//!   the list of frames and where each one's bitstream lies.
//! * [`fragment_headers`] is `WebPParseHeaders` on one frame's bytes, which is
//!   what each frame's decode starts with.
//!
//! The format's specification (RFC 9649) leaves much of this to the reader;
//! libwebp is what browsers and Pillow run, so its answers are the ones that
//! decide whether a file shows and how. Where the three disagree with each
//! other -- the `VP8X` chunk may be longer than ten bytes to the demuxer but
//! not to `WebPGetFeatures` -- the file must pass all three, as it must in
//! libwebp.

use alloc::vec::Vec;

use crate::{ImageError, ImageResult};

/// A chunk's fourcc and size.
const CHUNK_HEADER_SIZE: usize = 8;
/// "RIFF", its size, "WEBP".
const RIFF_HEADER_SIZE: usize = 12;
/// A fourcc.
const TAG_SIZE: usize = 4;
/// The `VP8X` chunk's payload.
const VP8X_CHUNK_SIZE: usize = 10;
/// The `ANIM` chunk's payload.
const ANIM_CHUNK_SIZE: usize = 6;
/// The `ANMF` chunk's payload before its frame's chunks.
const ANMF_CHUNK_SIZE: usize = 16;
/// The largest payload a chunk may declare: `~0 - CHUNK_HEADER_SIZE - 1`.
const MAX_CHUNK_PAYLOAD: u32 = u32::MAX - 8 - 1;
/// Canvases and frames must have fewer pixels than this.
const MAX_IMAGE_AREA: u64 = 1 << 32;
/// A VP8 frame's uncompressed header.
const VP8_FRAME_HEADER_SIZE: usize = 10;
/// A VP8L stream's header.
const VP8L_FRAME_HEADER_SIZE: usize = 5;

/// `VP8X` feature flags.
const ANIMATION_FLAG: u32 = 0x02;
const XMP_FLAG: u32 = 0x04;
const EXIF_FLAG: u32 = 0x08;
const ALPHA_FLAG: u32 = 0x10;
const ICCP_FLAG: u32 = 0x20;
const ALL_VALID_FLAGS: u32 = ANIMATION_FLAG | XMP_FLAG | EXIF_FLAG | ALPHA_FLAG | ICCP_FLAG;

/// libwebp's status codes, as far as they matter here: the file is short
/// (`VP8_STATUS_NOT_ENOUGH_DATA`), or wrong (`VP8_STATUS_BITSTREAM_ERROR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    NotEnoughData,
    Error(&'static str),
}

impl From<Status> for ImageError {
    fn from(status: Status) -> Self {
        match status {
            Status::NotEnoughData => Self::Truncated,
            Status::Error(what) => Self::Malformed(what),
        }
    }
}

fn le16(data: &[u8], at: usize) -> u32 {
    let byte = |i: usize| u32::from(data.get(at.saturating_add(i)).copied().unwrap_or(0));
    byte(0) | (byte(1) << 8)
}

fn le24(data: &[u8], at: usize) -> u32 {
    let byte = |i: usize| u32::from(data.get(at.saturating_add(i)).copied().unwrap_or(0));
    byte(0) | (byte(1) << 8) | (byte(2) << 16)
}

fn le32(data: &[u8], at: usize) -> u32 {
    let byte = |i: usize| u32::from(data.get(at.saturating_add(i)).copied().unwrap_or(0));
    byte(0) | (byte(1) << 8) | (byte(2) << 16) | (byte(3) << 24)
}

fn tag_is(data: &[u8], at: usize, tag: &[u8; 4]) -> bool {
    data.get(at..at.saturating_add(TAG_SIZE)) == Some(tag.as_slice())
}

// ---------------------------------------------------------------------------
// WebPGetFeatures, and the headers each frame's decode reads
// ---------------------------------------------------------------------------

/// What `WebPGetFeatures` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Features {
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub has_animation: bool,
}

/// Where a frame's bitstream is, as `WebPParseHeaders` finds it in the bytes
/// it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Headers {
    /// Where the bitstream starts: past the chunk headers.
    pub offset: usize,
    /// The bitstream chunk's declared size (or, for a raw stream, all of it).
    pub compressed_size: usize,
    pub is_lossless: bool,
    /// The last `ALPH` chunk's payload before the bitstream: offset and size.
    pub alpha: Option<(usize, usize)>,
}

impl Headers {
    /// No bitstream found: what `WebPGetFeatures` leaves when it answers from
    /// the `VP8X` chunk alone.
    const NONE: Self = Self {
        offset: 0,
        compressed_size: 0,
        is_lossless: false,
        alpha: None,
    };
}

/// `ParseHeadersInternal` (`src/dec/webp_dec.c`). With `have_all_data` false
/// it is `WebPGetFeatures`'s look at a file; true, `WebPParseHeaders`'s at the
/// bytes a decode was given.
///
/// The C function stops early in two ways: a `return`, which is final, and a
/// `goto ReturnWidthHeight`, after which a shortage in a file with a `VP8X`
/// chunk is forgiven when only the features were asked for -- the canvas is
/// what the caller wanted, and it has been read. The second is `stop` here.
#[allow(
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    reason = "one transcription of one C function, kept whole to be read against it; every offset is checked against the data's length before it is used"
)]
fn parse_headers(data: &[u8], have_all_data: bool) -> Result<(Features, Headers), Status> {
    if data.len() < RIFF_HEADER_SIZE {
        return Err(Status::NotEnoughData);
    }
    let remaining = |at: usize| data.len().saturating_sub(at);

    // ParseRIFF.
    let mut at = 0usize;
    let mut riff_size = 0u32;
    if tag_is(data, 0, b"RIFF") {
        if !tag_is(data, 8, b"WEBP") {
            return Err(Status::Error("a RIFF file that is not a WebP"));
        }
        let size = le32(data, 4);
        if (size as usize) < TAG_SIZE + CHUNK_HEADER_SIZE {
            return Err(Status::Error("a WebP too small to hold a chunk"));
        }
        if size > MAX_CHUNK_PAYLOAD {
            return Err(Status::Error("a WebP larger than RIFF allows"));
        }
        if have_all_data && size as usize > data.len() - CHUNK_HEADER_SIZE {
            return Err(Status::NotEnoughData);
        }
        riff_size = size;
        at = RIFF_HEADER_SIZE;
    }
    let found_riff = riff_size > 0;

    // ParseVP8X.
    if remaining(at) < CHUNK_HEADER_SIZE {
        return Err(Status::NotEnoughData);
    }
    let (found_vp8x, canvas_width, canvas_height, flags) = if tag_is(data, at, b"VP8X") {
        if le32(data, at + TAG_SIZE) as usize != VP8X_CHUNK_SIZE {
            return Err(Status::Error("a VP8X chunk of the wrong size"));
        }
        if remaining(at) < CHUNK_HEADER_SIZE + VP8X_CHUNK_SIZE {
            return Err(Status::NotEnoughData);
        }
        let flags = le32(data, at + 8);
        let width = 1 + le24(data, at + 12);
        let height = 1 + le24(data, at + 15);
        if u64::from(width) * u64::from(height) >= MAX_IMAGE_AREA {
            return Err(Status::Error("a WebP canvas too large"));
        }
        at += CHUNK_HEADER_SIZE + VP8X_CHUNK_SIZE;
        (true, width, height, flags)
    } else {
        (false, 0, 0, 0)
    };
    let has_animation = flags & ANIMATION_FLAG != 0;
    if !found_riff && found_vp8x {
        return Err(Status::Error("a VP8X chunk outside a RIFF file"));
    }
    let flagged_alpha = flags & ALPHA_FLAG != 0;
    let canvas = |found_alpha: bool| {
        (
            Features {
                width: canvas_width,
                height: canvas_height,
                has_alpha: flagged_alpha || found_alpha,
                has_animation,
            },
            Headers::NONE,
        )
    };
    let stop = |status: Status, found_alpha: bool| {
        if status == Status::NotEnoughData && found_vp8x && !have_all_data {
            Ok(canvas(found_alpha))
        } else {
            Err(status)
        }
    };
    if found_vp8x && has_animation && !have_all_data {
        return Ok(canvas(false));
    }
    if remaining(at) < TAG_SIZE {
        return stop(Status::NotEnoughData, false);
    }

    // ParseOptionalChunks: every chunk before the bitstream's, remembering the
    // last `ALPH`. The running total is a `uint32_t` in libwebp, and wraps.
    let mut alpha: Option<(usize, usize)> = None;
    if (found_riff && found_vp8x) || (!found_riff && !found_vp8x && tag_is(data, at, b"ALPH")) {
        let mut total = (TAG_SIZE + CHUNK_HEADER_SIZE + VP8X_CHUNK_SIZE) as u32;
        loop {
            if remaining(at) < CHUNK_HEADER_SIZE {
                return stop(Status::NotEnoughData, alpha.is_some());
            }
            let size = le32(data, at + TAG_SIZE);
            if size > MAX_CHUNK_PAYLOAD {
                return Err(Status::Error("a chunk larger than RIFF allows"));
            }
            // At most MAX_CHUNK_PAYLOAD + 9 = u32::MAX, so this cannot wrap.
            let disk = (CHUNK_HEADER_SIZE as u32 + size + 1) & !1;
            total = total.wrapping_add(disk);
            if riff_size > 0 && total > riff_size {
                return Err(Status::Error("a chunk running past its RIFF file"));
            }
            if tag_is(data, at, b"VP8 ") || tag_is(data, at, b"VP8L") {
                break;
            }
            if (remaining(at) as u64) < u64::from(disk) {
                return stop(Status::NotEnoughData, alpha.is_some());
            }
            if tag_is(data, at, b"ALPH") {
                alpha = Some((at + CHUNK_HEADER_SIZE, size as usize));
            }
            at += disk as usize;
        }
    }
    let found_alpha = alpha.is_some();

    // ParseVP8Header.
    if remaining(at) < CHUNK_HEADER_SIZE {
        return stop(Status::NotEnoughData, found_alpha);
    }
    let is_vp8 = tag_is(data, at, b"VP8 ");
    let is_vp8l = tag_is(data, at, b"VP8L");
    let (compressed_size, is_lossless) = if is_vp8 || is_vp8l {
        let size = le32(data, at + TAG_SIZE);
        let minimal = (TAG_SIZE + CHUNK_HEADER_SIZE) as u32;
        if riff_size >= minimal && size > riff_size - minimal {
            return Err(Status::Error("a bitstream chunk larger than its RIFF file"));
        }
        if have_all_data && size as usize > remaining(at) - CHUNK_HEADER_SIZE {
            return stop(Status::NotEnoughData, found_alpha);
        }
        at += CHUNK_HEADER_SIZE;
        (size as usize, is_vp8l)
    } else {
        // A bare bitstream: libwebp takes it for lossless if it starts like
        // a VP8L stream (`VP8LCheckSignature`).
        let rest = data.get(at..).unwrap_or(&[]);
        let lossless = rest.len() >= VP8L_FRAME_HEADER_SIZE
            && rest.first() == Some(&super::lossless::SIGNATURE)
            && rest.get(4).is_some_and(|byte| byte >> 5 == 0);
        (rest.len(), lossless)
    };
    if compressed_size as u64 > u64::from(MAX_CHUNK_PAYLOAD) {
        return Err(Status::Error("a bitstream larger than RIFF allows"));
    }

    // VP8GetInfo or VP8LGetInfo, on everything from the bitstream on.
    let stream = data.get(at..).unwrap_or(&[]);
    let (image_width, image_height, has_alpha) = if is_lossless {
        if stream.len() < VP8L_FRAME_HEADER_SIZE {
            return stop(Status::NotEnoughData, found_alpha);
        }
        let (width, height) = super::lossless::dimensions(stream)
            .map_err(|_| Status::Error("a VP8L stream with a broken header"))?;
        // The stream's own flag replaces the VP8X chunk's.
        (width, height, super::lossless::alpha_hint(stream))
    } else {
        if stream.len() < VP8_FRAME_HEADER_SIZE {
            return stop(Status::NotEnoughData, found_alpha);
        }
        let (width, height) = super::lossy::info(stream, compressed_size)
            .map_err(|_| Status::Error("a VP8 frame with a broken header"))?;
        (width, height, flagged_alpha)
    };
    if found_vp8x && (canvas_width != image_width || canvas_height != image_height) {
        return Err(Status::Error("a picture a different size from its canvas"));
    }
    Ok((
        Features {
            width: image_width,
            height: image_height,
            has_alpha: has_alpha || found_alpha,
            has_animation,
        },
        Headers {
            offset: at,
            compressed_size,
            is_lossless,
            alpha,
        },
    ))
}

/// `WebPGetFeatures` of a file.
pub(super) fn features(data: &[u8]) -> ImageResult<Features> {
    Ok(parse_headers(data, false)?.0)
}

/// What a frame's decode finds in its bytes (`WebPDecode`: `WebPGetFeatures`,
/// then `WebPParseHeaders`), refusing an animation as libwebp does.
pub(super) fn fragment_headers(fragment: &[u8]) -> ImageResult<Headers> {
    // WebPDecode treats a shortage here as an error, not as a wait for more.
    let (features, _) = parse_headers(fragment, false)?;
    if features.has_animation {
        return Err(ImageError::Malformed(
            "an animation where a frame was expected",
        ));
    }
    let (_, headers) = parse_headers(fragment, true)?;
    Ok(headers)
}

// ---------------------------------------------------------------------------
// WebPDemux
// ---------------------------------------------------------------------------

/// How a frame leaves its rectangle for the next (`WebPMuxAnimDispose`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dispose {
    None,
    Background,
}

/// How a frame meets what is under it (`WebPMuxAnimBlend`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Blend {
    Blend,
    NoBlend,
}

/// A frame, as the demuxer records it (`Frame` in `demux.c`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub x_offset: u32,
    pub y_offset: u32,
    /// The bitstream's own width and height, which replace the `ANMF`
    /// chunk's as libwebp's demuxer replaces them.
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub duration: u32,
    pub dispose: Dispose,
    pub blend: Blend,
    frame_num: u32,
    complete: bool,
    /// The bitstream chunk, header included: offset and size.
    image: (usize, usize),
    /// The `ALPH` chunk, header included, if there is one.
    alpha: (usize, usize),
}

impl Frame {
    const fn new() -> Self {
        Self {
            x_offset: 0,
            y_offset: 0,
            width: 0,
            height: 0,
            has_alpha: false,
            duration: 0,
            dispose: Dispose::None,
            blend: Blend::Blend,
            frame_num: 0,
            complete: false,
            image: (0, 0),
            alpha: (0, 0),
        }
    }

    /// The bytes a frame's decode is given: from its `ALPH` chunk if it has
    /// one, through its bitstream chunk (`GetFramePayload`).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "offsets and sizes are within the file, checked as the frame was stored"
    )]
    pub(super) fn fragment<'d>(&self, data: &'d [u8]) -> &'d [u8] {
        let (image_offset, image_size) = self.image;
        let (mut start, mut size) = (image_offset, image_size);
        let (alpha_offset, alpha_size) = self.alpha;
        if alpha_size > 0 {
            let between = if image_offset > 0 {
                image_offset.saturating_sub(alpha_offset + alpha_size)
            } else {
                0
            };
            start = alpha_offset;
            size += alpha_size + between;
        }
        data.get(start..start.saturating_add(size)).unwrap_or(&[])
    }
}

/// Where parsing is (`WebPDemuxState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    ParsingHeader,
    ParsedHeader,
    Done,
}

/// How a parse step ended (`ParseStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parse {
    Ok,
    NeedMoreData,
    Error(&'static str),
}

/// The demuxer's view of the bytes (`MemBuffer`).
struct Mem<'d> {
    buf: &'d [u8],
    start: usize,
    end: usize,
    riff_end: usize,
}

impl Mem<'_> {
    const fn available(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether `size` bytes from here run past the RIFF chunk.
    const fn size_is_invalid(&self, size: u64) -> bool {
        size > self.riff_end.saturating_sub(self.start) as u64
    }

    fn skip(&mut self, n: usize) {
        self.start = self.start.saturating_add(n);
    }

    fn rewind(&mut self, n: usize) {
        self.start = self.start.saturating_sub(n);
    }

    fn read_le32(&mut self) -> u32 {
        let value = le32(self.buf, self.start);
        self.skip(4);
        value
    }

    fn read_le24(&mut self) -> u32 {
        let value = le24(self.buf, self.start);
        self.skip(3);
        value
    }

    fn read_le16(&mut self) -> u32 {
        let value = le16(self.buf, self.start);
        self.skip(2);
        value
    }

    fn read_byte(&mut self) -> u8 {
        let value = self.buf.get(self.start).copied().unwrap_or(0);
        self.skip(1);
        value
    }
}

/// A parsed WebP file (`WebPDemuxer`).
pub(super) struct Demuxer<'d> {
    mem: Mem<'d>,
    state: State,
    is_ext_format: bool,
    pub feature_flags: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub loop_count: u32,
    pub frames: Vec<Frame>,
}

impl<'d> Demuxer<'d> {
    /// `WebPDemux` of a whole file: the RIFF header, then the chunks, then the
    /// demuxer's validity checks. A file shorter than its RIFF header says is
    /// refused, as libwebp refuses it without `allow_partial`.
    pub(super) fn read(data: &'d [u8]) -> ImageResult<Self> {
        // ReadHeader.
        if data.len() < RIFF_HEADER_SIZE + CHUNK_HEADER_SIZE {
            return Err(ImageError::Truncated);
        }
        if !tag_is(data, 0, b"RIFF") || !tag_is(data, 8, b"WEBP") {
            return Err(ImageError::Malformed("not a RIFF WEBP file"));
        }
        let riff_size = le32(data, 4);
        if (riff_size as usize) < CHUNK_HEADER_SIZE || riff_size > MAX_CHUNK_PAYLOAD {
            return Err(ImageError::Malformed("a WebP with an impossible RIFF size"));
        }
        let riff_end = (riff_size as usize).saturating_add(CHUNK_HEADER_SIZE);
        // Nothing past the RIFF chunk is read; a file shorter than it is
        // partial, which a whole-file decode does not accept.
        let end = data.len().min(riff_end);
        if end < riff_end {
            return Err(ImageError::Truncated);
        }
        let mut dmux = Self {
            mem: Mem {
                buf: data,
                start: RIFF_HEADER_SIZE,
                end,
                riff_end,
            },
            state: State::ParsingHeader,
            is_ext_format: false,
            feature_flags: 0,
            canvas_width: 0,
            canvas_height: 0,
            loop_count: 1,
            frames: Vec::new(),
        };
        let first = data.get(RIFF_HEADER_SIZE..RIFF_HEADER_SIZE + TAG_SIZE);
        let (status, simple) = match first {
            Some(b"VP8 " | b"VP8L") => (dmux.parse_single_image(), true),
            Some(b"VP8X") => (dmux.parse_vp8x(), false),
            _ => {
                return Err(ImageError::Malformed(
                    "a WebP whose first chunk is not a picture",
                ));
            }
        };
        let status = match status {
            Parse::Ok => {
                dmux.state = State::Done;
                Parse::Ok
            }
            // The file is whole, so wanting more data is an error.
            Parse::NeedMoreData => Parse::Error("a WebP whose chunks run out"),
            error @ Parse::Error(_) => error,
        };
        if let Parse::Error(what) = status {
            return Err(ImageError::Malformed(what));
        }
        let valid = if simple {
            dmux.is_valid_simple_format()
        } else {
            dmux.is_valid_extended_format()
        };
        if !valid {
            return Err(ImageError::Malformed("a WebP the demuxer finds invalid"));
        }
        Ok(dmux)
    }

    /// Store a frame's image chunks (`StoreFrame`): an optional `ALPH`, then
    /// `VP8 ` or `VP8L`, ending at the first chunk that is neither.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "sizes are checked against the RIFF chunk before they move the position"
    )]
    fn store_frame(&mut self, frame_num: u32, min_size: usize, frame: &mut Frame) -> Parse {
        let mem = &mut self.mem;
        let mut alpha_chunks = 0;
        let mut image_chunks = 0;
        if mem.available() < CHUNK_HEADER_SIZE || mem.available() < min_size {
            return Parse::NeedMoreData;
        }
        let mut status = Parse::Ok;
        loop {
            let chunk_start = mem.start;
            let fourcc = mem.read_le32().to_le_bytes();
            let payload_size = mem.read_le32();
            if payload_size > MAX_CHUNK_PAYLOAD {
                return Parse::Error("a chunk larger than RIFF allows");
            }
            let padded = u64::from(payload_size) + u64::from(payload_size & 1);
            let available = (padded as usize).min(mem.available());
            let chunk_size = CHUNK_HEADER_SIZE + available;
            if mem.size_is_invalid(padded) {
                return Parse::Error("a chunk running past its RIFF file");
            }
            if padded > mem.available() as u64 {
                status = Parse::NeedMoreData;
            }
            let mut done = false;
            match &fourcc {
                b"ALPH" if alpha_chunks == 0 => {
                    alpha_chunks += 1;
                    frame.alpha = (chunk_start, chunk_size);
                    frame.has_alpha = true;
                    frame.frame_num = frame_num;
                    mem.skip(available);
                }
                b"VP8L" if alpha_chunks > 0 => {
                    return Parse::Error("a VP8L frame with an ALPH chunk");
                }
                b"VP8 " | b"VP8L" if image_chunks == 0 => {
                    let chunk = mem
                        .buf
                        .get(chunk_start..chunk_start + chunk_size)
                        .unwrap_or(&[]);
                    let features = match parse_headers(chunk, false) {
                        Ok((features, _)) => features,
                        Err(Status::NotEnoughData) if status == Parse::NeedMoreData => {
                            return Parse::NeedMoreData;
                        }
                        Err(_) => return Parse::Error("a frame whose bitstream header is broken"),
                    };
                    image_chunks += 1;
                    frame.image = (chunk_start, chunk_size);
                    frame.width = features.width;
                    frame.height = features.height;
                    frame.has_alpha |= features.has_alpha;
                    frame.frame_num = frame_num;
                    frame.complete = status == Parse::Ok;
                    mem.skip(available);
                }
                _ => {
                    mem.rewind(CHUNK_HEADER_SIZE);
                    done = true;
                }
            }
            if mem.start == mem.riff_end {
                done = true;
            } else if mem.available() < CHUNK_HEADER_SIZE {
                status = Parse::NeedMoreData;
            }
            if done || status != Parse::Ok {
                return status;
            }
        }
    }

    /// Add a frame, if the one before it is complete (`AddFrame`).
    fn add_frame(&mut self, frame: Frame) -> bool {
        if self.frames.last().is_some_and(|last| !last.complete) {
            return false;
        }
        self.frames.push(frame);
        true
    }

    /// `ParseSingleImage`: a picture that is not an animation frame.
    fn parse_single_image(&mut self) -> Parse {
        if !self.frames.is_empty() {
            return Parse::Error("a second picture in a still WebP");
        }
        if self.mem.size_is_invalid(CHUNK_HEADER_SIZE as u64) {
            return Parse::Error("a chunk running past its RIFF file");
        }
        if self.mem.available() < CHUNK_HEADER_SIZE {
            return Parse::NeedMoreData;
        }
        let mut frame = Frame::new();
        let status = self.store_frame(1, 0, &mut frame);
        if matches!(status, Parse::Error(_)) {
            return status;
        }
        // "Clear any alpha when the alpha flag is missing."
        if self.feature_flags & ALPHA_FLAG == 0 && frame.alpha.1 > 0 {
            frame.alpha = (0, 0);
            frame.has_alpha = false;
        }
        // A simple file's canvas is its picture, and its alpha flag the
        // picture's.
        if !self.is_ext_format && frame.width > 0 && frame.height > 0 {
            self.state = State::ParsedHeader;
            self.canvas_width = frame.width;
            self.canvas_height = frame.height;
            if frame.has_alpha {
                self.feature_flags |= ALPHA_FLAG;
            }
        }
        if !self.add_frame(frame) {
            return Parse::Error("a frame after an incomplete one");
        }
        status
    }

    /// `ParseVP8X`: the extended header, then its chunks.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the chunk's size is at least ten, checked first"
    )]
    fn parse_vp8x(&mut self) -> Parse {
        let mem = &mut self.mem;
        if mem.available() < CHUNK_HEADER_SIZE {
            return Parse::NeedMoreData;
        }
        self.is_ext_format = true;
        mem.skip(TAG_SIZE);
        let size = mem.read_le32();
        if size > MAX_CHUNK_PAYLOAD {
            return Parse::Error("a VP8X chunk larger than RIFF allows");
        }
        if (size as usize) < VP8X_CHUNK_SIZE {
            return Parse::Error("a VP8X chunk too small");
        }
        let padded = size as usize + (size as usize & 1);
        if mem.size_is_invalid(padded as u64) {
            return Parse::Error("a VP8X chunk running past its RIFF file");
        }
        if mem.available() < padded {
            return Parse::NeedMoreData;
        }
        self.feature_flags = u32::from(mem.read_byte());
        mem.skip(3);
        self.canvas_width = 1 + mem.read_le24();
        self.canvas_height = 1 + mem.read_le24();
        if u64::from(self.canvas_width) * u64::from(self.canvas_height) >= MAX_IMAGE_AREA {
            return Parse::Error("a WebP canvas too large");
        }
        mem.skip(padded - VP8X_CHUNK_SIZE);
        self.state = State::ParsedHeader;
        if mem.size_is_invalid(CHUNK_HEADER_SIZE as u64) {
            return Parse::Error("a VP8X file with nothing after its header");
        }
        if mem.available() < CHUNK_HEADER_SIZE {
            return Parse::NeedMoreData;
        }
        self.parse_vp8x_chunks()
    }

    /// `ParseVP8XChunks`: every chunk after the `VP8X` one.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "sizes are checked against the RIFF chunk before they move the position"
    )]
    fn parse_vp8x_chunks(&mut self) -> Parse {
        let is_animation = self.feature_flags & ANIMATION_FLAG != 0;
        let mut anim_chunks = 0;
        loop {
            let fourcc = self.mem.read_le32().to_le_bytes();
            let chunk_size = self.mem.read_le32();
            if chunk_size > MAX_CHUNK_PAYLOAD {
                return Parse::Error("a chunk larger than RIFF allows");
            }
            let padded = u64::from(chunk_size) + u64::from(chunk_size & 1);
            if self.mem.size_is_invalid(padded) {
                return Parse::Error("a chunk running past its RIFF file");
            }
            let mut status = Parse::Ok;
            let mut skip = false;
            match &fourcc {
                b"VP8X" => return Parse::Error("a second VP8X chunk"),
                b"ALPH" | b"VP8 " | b"VP8L" => {
                    if anim_chunks > 0 || is_animation {
                        return Parse::Error("a picture outside the frames of an animation");
                    }
                    self.mem.rewind(CHUNK_HEADER_SIZE);
                    status = self.parse_single_image();
                }
                b"ANIM" => {
                    if padded < ANIM_CHUNK_SIZE as u64 {
                        return Parse::Error("an ANIM chunk too small");
                    }
                    if (self.mem.available() as u64) < padded {
                        status = Parse::NeedMoreData;
                    } else if anim_chunks == 0 {
                        anim_chunks += 1;
                        // The background colour, which libwebp's animation
                        // decoder -- like the browsers -- does not use.
                        self.mem.skip(4);
                        self.loop_count = self.mem.read_le16();
                        self.mem.skip(padded as usize - ANIM_CHUNK_SIZE);
                    } else {
                        skip = true;
                    }
                }
                b"ANMF" => {
                    if anim_chunks == 0 {
                        return Parse::Error("an ANMF chunk before the ANIM chunk");
                    }
                    status = self.parse_animation_frame(padded);
                }
                _ => skip = true,
            }
            if skip {
                if padded <= self.mem.available() as u64 {
                    self.mem.skip(padded as usize);
                } else {
                    status = Parse::NeedMoreData;
                }
            }
            if self.mem.start == self.mem.riff_end {
                return status;
            } else if self.mem.available() < CHUNK_HEADER_SIZE {
                status = Parse::NeedMoreData;
            }
            if status != Parse::Ok {
                return status;
            }
        }
    }

    /// `ParseAnimationFrame`: an `ANMF` chunk and the image chunks in it.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the chunk is at least ANMF_CHUNK_SIZE, checked first, and the fields are 24 bits"
    )]
    fn parse_animation_frame(&mut self, frame_chunk_size: u64) -> Parse {
        let is_animation = self.feature_flags & ANIMATION_FLAG != 0;
        // NewFrame.
        if self.mem.size_is_invalid(ANMF_CHUNK_SIZE as u64) {
            return Parse::Error("an ANMF chunk running past its RIFF file");
        }
        if frame_chunk_size < ANMF_CHUNK_SIZE as u64 {
            return Parse::Error("an ANMF chunk too small");
        }
        if self.mem.available() < ANMF_CHUNK_SIZE {
            return Parse::NeedMoreData;
        }
        let payload = (frame_chunk_size - ANMF_CHUNK_SIZE as u64) as usize;
        let mut frame = Frame::new();
        frame.x_offset = 2 * self.mem.read_le24();
        frame.y_offset = 2 * self.mem.read_le24();
        frame.width = 1 + self.mem.read_le24();
        frame.height = 1 + self.mem.read_le24();
        frame.duration = self.mem.read_le24();
        let bits = self.mem.read_byte();
        frame.dispose = if bits & 1 != 0 {
            Dispose::Background
        } else {
            Dispose::None
        };
        frame.blend = if bits & 2 != 0 {
            Blend::NoBlend
        } else {
            Blend::Blend
        };
        if u64::from(frame.width) * u64::from(frame.height) >= MAX_IMAGE_AREA {
            return Parse::Error("an animation frame too large");
        }
        let start = self.mem.start;
        let frame_num = u32::try_from(self.frames.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        let mut status = self.store_frame(frame_num, payload, &mut frame);
        if !matches!(status, Parse::Error(_)) && self.mem.start - start > payload {
            status = Parse::Error("an animation frame running past its ANMF chunk");
        }
        if !matches!(status, Parse::Error(_))
            && is_animation
            && frame.frame_num > 0
            && !self.add_frame(frame)
        {
            status = Parse::Error("a frame after an incomplete one");
        }
        status
    }

    /// `IsValidSimpleFormat`.
    fn is_valid_simple_format(&self) -> bool {
        if self.state == State::ParsingHeader {
            return true;
        }
        if self.canvas_width == 0 || self.canvas_height == 0 {
            return false;
        }
        match self.frames.first() {
            None => self.state != State::Done,
            Some(frame) => frame.width > 0 && frame.height > 0,
        }
    }

    /// `IsValidExtendedFormat`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "offsets and sizes are 24-bit fields plus one"
    )]
    fn is_valid_extended_format(&self) -> bool {
        let is_animation = self.feature_flags & ANIMATION_FLAG != 0;
        if self.state == State::ParsingHeader {
            return true;
        }
        if self.canvas_width == 0 || self.canvas_height == 0 {
            return false;
        }
        if self.state == State::Done && self.frames.is_empty() {
            return false;
        }
        if self.feature_flags & !ALL_VALID_FLAGS != 0 {
            return false;
        }
        for (i, frame) in self.frames.iter().enumerate() {
            let (image_offset, image_size) = frame.image;
            let (alpha_offset, alpha_size) = frame.alpha;
            if !is_animation && frame.frame_num > 1 {
                return false;
            }
            if frame.complete {
                if alpha_size == 0 && image_size == 0 {
                    return false;
                }
                if alpha_size > 0 && alpha_offset > image_offset {
                    return false;
                }
                if frame.width == 0 || frame.height == 0 {
                    return false;
                }
            } else {
                if self.state == State::Done {
                    return false;
                }
                if alpha_size > 0 && image_size > 0 && alpha_offset > image_offset {
                    return false;
                }
                if i + 1 < self.frames.len() {
                    return false;
                }
            }
            if frame.width > 0 && frame.height > 0 {
                let fits = if is_animation {
                    u64::from(frame.width) + u64::from(frame.x_offset)
                        <= u64::from(self.canvas_width)
                        && u64::from(frame.height) + u64::from(frame.y_offset)
                            <= u64::from(self.canvas_height)
                } else {
                    frame.x_offset == 0
                        && frame.y_offset == 0
                        && frame.width == self.canvas_width
                        && frame.height == self.canvas_height
                };
                if !fits {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests"
    )]

    use super::*;
    use alloc::vec;

    const LOSSLESS: &[u8] = include_bytes!("../../tests/data/webp_lossless_1x1.webp");
    const LOSSY: &[u8] = include_bytes!("../../tests/data/webp_lossy_1x1.webp");

    /// The one chunk's payload of a simple file.
    fn payload(file: &[u8]) -> &[u8] {
        &file[20..20 + le32(file, 16) as usize]
    }

    fn chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body = chunks.concat();
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
        out.extend_from_slice(b"WEBP");
        out.extend_from_slice(&body);
        out
    }

    fn vp8x(flags: u8, width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![flags, 0, 0, 0];
        payload.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        chunk(b"VP8X", &payload)
    }

    fn anim() -> Vec<u8> {
        chunk(b"ANIM", &[0; 6])
    }

    fn anmf(x: u32, y: u32, size: (u32, u32), inner: &[Vec<u8>]) -> Vec<u8> {
        let mut payload = Vec::new();
        for value in [x / 2, y / 2, size.0 - 1, size.1 - 1, 100] {
            payload.extend_from_slice(&value.to_le_bytes()[..3]);
        }
        payload.push(0);
        payload.extend_from_slice(&inner.concat());
        chunk(b"ANMF", &payload)
    }

    fn lossless() -> Vec<u8> {
        chunk(b"VP8L", payload(LOSSLESS))
    }

    fn lossy() -> Vec<u8> {
        chunk(b"VP8 ", payload(LOSSY))
    }

    const ALPHA: u8 = ALPHA_FLAG as u8;
    const ANIMATED: u8 = ANIMATION_FLAG as u8;

    #[test]
    fn a_simple_file_is_one_frame_the_size_of_its_picture() {
        assert_eq!(riff(&[lossless()]), LOSSLESS, "the builder is sound");
        for file in [riff(&[lossless()]), riff(&[lossy()])] {
            let found = features(&file).unwrap();
            assert_eq!(
                (found.width, found.height, found.has_animation),
                (1, 1, false)
            );
            let demux = Demuxer::read(&file).unwrap();
            assert_eq!((demux.canvas_width, demux.canvas_height), (1, 1));
            assert_eq!(demux.frames.len(), 1);
            assert_eq!(demux.frames[0].fragment(&file), &file[12..]);
        }
    }

    #[test]
    fn flags_the_format_does_not_define_make_the_demuxer_refuse_a_file() {
        assert!(Demuxer::read(&riff(&[vp8x(0, 1, 1), lossless()])).is_ok());
        for flag in [0x01, 0x40, 0x80] {
            let file = riff(&[vp8x(flag, 1, 1), lossless()]);
            assert!(
                features(&file).is_ok(),
                "{flag:#x}: WebPGetFeatures ignores it"
            );
            assert!(Demuxer::read(&file).is_err(), "{flag:#x}");
        }
        // The three bytes after the flags are read by neither.
        let mut reserved = riff(&[vp8x(0, 1, 1), lossless()]);
        reserved[21] = 0xFF;
        assert!(features(&reserved).is_ok() && Demuxer::read(&reserved).is_ok());
    }

    #[test]
    fn a_long_vp8x_chunk_passes_the_demuxer_and_not_webpgetfeatures() {
        let file = riff(&[chunk(b"VP8X", &[0; 12]), lossless()]);
        assert!(Demuxer::read(&file).is_ok());
        assert!(matches!(features(&file), Err(ImageError::Malformed(_))));
    }

    #[test]
    fn an_alph_chunk_under_no_alpha_flag_is_dropped_by_the_demuxer_only() {
        let alph = chunk(b"ALPH", &[0, 0xFF]);
        let flagged = riff(&[vp8x(ALPHA, 1, 1), alph.clone(), lossy()]);
        let demux = Demuxer::read(&flagged).unwrap();
        assert!(demux.frames[0].has_alpha);
        let headers = fragment_headers(demux.frames[0].fragment(&flagged)).unwrap();
        assert_eq!(
            headers.alpha,
            Some((8, 2)),
            "the ALPH payload, in the fragment"
        );

        let unflagged = riff(&[vp8x(0, 1, 1), alph, lossy()]);
        let demux = Demuxer::read(&unflagged).unwrap();
        assert!(!demux.frames[0].has_alpha);
        let headers = fragment_headers(demux.frames[0].fragment(&unflagged)).unwrap();
        assert_eq!(headers.alpha, None);
        // WebPGetFeatures, reading the file itself, sees it either way.
        assert!(features(&unflagged).unwrap().has_alpha);
    }

    #[test]
    fn a_simple_file_ignores_an_alph_chunk_after_its_picture() {
        let file = riff(&[lossy(), chunk(b"ALPH", &[0, 0])]);
        let demux = Demuxer::read(&file).unwrap();
        assert!(!demux.frames[0].has_alpha);
        assert_eq!(demux.frames[0].fragment(&file), &lossy()[..]);
    }

    #[test]
    fn a_file_cut_short_keeps_its_size_once_its_headers_are_in() {
        let file = riff(&[vp8x(ALPHA, 1, 1), chunk(b"ALPH", &[0, 0xFF]), lossy()]);
        for cut in 0..file.len() {
            let short = &file[..cut];
            assert!(
                matches!(Demuxer::read(short), Err(ImageError::Truncated)),
                "{cut}"
            );
            // Past the VP8X chunk, WebPGetFeatures answers from it.
            match features(short) {
                Ok(found) => assert!(cut >= 30 && (found.width, found.height) == (1, 1), "{cut}"),
                Err(e) => assert!(cut < 30 && e == ImageError::Truncated, "{cut}"),
            }
        }
        // Without one, once the picture's own header is in.
        let simple = riff(&[lossy()]);
        assert!(features(&simple[..29]).is_err());
        assert_eq!(features(&simple[..30]).unwrap().width, 1);
    }

    #[test]
    fn bytes_past_the_riff_chunk_are_ignored_and_a_few_inside_it_are_not() {
        let mut after = riff(&[lossless()]);
        after.extend_from_slice(b"trailing");
        assert!(Demuxer::read(&after).is_ok());
        assert!(Demuxer::read(&riff(&[lossless(), vec![0; 4]])).is_err());
        assert!(Demuxer::read(&riff(&[lossless(), chunk(b"JUNK", b"12")])).is_ok());
    }

    #[test]
    fn a_second_picture_or_vp8x_chunk_is_refused() {
        assert!(Demuxer::read(&riff(&[vp8x(0, 1, 1), lossless(), lossless()])).is_err());
        assert!(Demuxer::read(&riff(&[vp8x(0, 1, 1), vp8x(0, 1, 1), lossless()])).is_err());
        assert!(
            features(&riff(&[vp8x(0, 2, 1), lossless()])).is_err(),
            "a picture that is not the canvas's size"
        );
    }

    #[test]
    fn an_animation_is_frames_after_an_anim_chunk_inside_the_canvas() {
        let inside = riff(&[
            vp8x(ANIMATED, 3, 3),
            anim(),
            anmf(2, 2, (1, 1), &[lossless()]),
        ]);
        let demux = Demuxer::read(&inside).unwrap();
        assert_eq!(demux.frames.len(), 1);
        assert_eq!((demux.frames[0].x_offset, demux.frames[0].y_offset), (2, 2));
        let outside = riff(&[
            vp8x(ANIMATED, 3, 3),
            anim(),
            anmf(4, 2, (1, 1), &[lossless()]),
        ]);
        assert!(Demuxer::read(&outside).is_err());
        // The picture's size wins over the ANMF chunk's, which may not fit.
        let declared = riff(&[
            vp8x(ANIMATED, 3, 3),
            anim(),
            anmf(2, 2, (9, 9), &[lossless()]),
        ]);
        let demux = Demuxer::read(&declared).unwrap();
        assert_eq!((demux.frames[0].width, demux.frames[0].height), (1, 1));
        // No ANIM chunk first; a still picture among the frames; frames with
        // no animation flag, which are read and dropped, leaving nothing.
        let frame = anmf(0, 0, (1, 1), &[lossless()]);
        assert!(Demuxer::read(&riff(&[vp8x(ANIMATED, 1, 1), frame.clone()])).is_err());
        assert!(Demuxer::read(&riff(&[vp8x(ANIMATED, 1, 1), anim(), lossless()])).is_err());
        assert!(Demuxer::read(&riff(&[vp8x(0, 1, 1), anim(), frame])).is_err());
        // WebPGetFeatures answers an animation from its VP8X chunk alone.
        let found = features(&inside).unwrap();
        assert_eq!(
            (found.width, found.has_animation, found.has_alpha),
            (3, true, false)
        );
    }

    #[test]
    fn the_optional_chunks_running_total_wraps_as_libwebps_does() {
        // A chunk declaring nearly 4 GiB: libwebp's running total, a 32-bit
        // number, wraps past zero, so WebPGetFeatures sees a file merely cut
        // short -- which after a VP8X chunk it forgives -- where the demuxer
        // sees a chunk running past its RIFF file.
        let mut huge = chunk(b"JUNK", &[]);
        huge[4..8].copy_from_slice(&0xFFFF_FFF0_u32.to_le_bytes());
        let file = riff(&[vp8x(0, 1, 1), huge, lossless()]);
        assert_eq!(features(&file).unwrap().width, 1);
        assert!(Demuxer::read(&file).is_err());
    }
}
