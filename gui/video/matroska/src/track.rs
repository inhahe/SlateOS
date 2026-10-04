//! A track's description: the `TrackEntry` element, read as RFC 9559 lays it
//! out and as FFmpeg accepts it (`matroska_parse_tracks`).

use std::borrow::Cow;
use std::io::{Read, Seek};

use crate::ebml::{Header, Id, MAX_BINARY, Reader};
use crate::{Error, ids};

/// What a track carries: RFC 9559's `TrackType`, the kinds FFmpeg reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    /// `metadata` (0x21).
    Metadata,
}

impl TrackKind {
    /// `None` for the kinds FFmpeg ignores the tracks of: complex, logo,
    /// buttons, control, and anything unknown.
    fn from_type(t: u64) -> Option<Self> {
        match t {
            1 => Some(Self::Video),
            2 => Some(Self::Audio),
            0x11 => Some(Self::Subtitle),
            0x21 => Some(Self::Metadata),
            _ => None,
        }
    }

    /// Whether a codec ID's first letter fits this kind: FFmpeg ignores a
    /// track whose codec ID says it is something else.
    fn fits(self, codec_id: &[u8]) -> bool {
        matches!(
            (self, codec_id.first()),
            (Self::Video, Some(b'V'))
                | (Self::Audio, Some(b'A'))
                | (Self::Subtitle | Self::Metadata, Some(b'D' | b'S'))
        )
    }
}

/// The codecs WebM allows, by codec ID; anything else is [`Codec::Other`],
/// its ID in [`Track::codec_id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Vp8,
    Vp9,
    Av1,
    Opus,
    Vorbis,
    WebVtt,
    Other,
}

impl Codec {
    fn from_id(id: &[u8]) -> Self {
        match id {
            b"V_VP8" => Self::Vp8,
            b"V_VP9" => Self::Vp9,
            b"V_AV1" => Self::Av1,
            b"A_OPUS" => Self::Opus,
            b"A_VORBIS" => Self::Vorbis,
            _ if id.starts_with(b"D_WEBVTT") => Self::WebVtt,
            _ => Self::Other,
        }
    }
}

/// A video track's picture: `Video`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Video {
    pub pixel_width: u64,
    pub pixel_height: u64,
    /// Samples cropped from each side: left, top, right, bottom.
    pub crop: [u64; 4],
    /// The size to show the picture at, in `display_unit`s; in pixels, the
    /// cropped picture's unless the file says otherwise. `None` when the
    /// file is silent and the unit is not pixels.
    pub display_width: Option<u64>,
    pub display_height: Option<u64>,
    /// 0 pixels, 1 centimetres, 2 inches, 3 an aspect ratio, 4 unknown.
    pub display_unit: u64,
    /// 1 when each block's `BlockAdditional` with ID 1 carries the alpha
    /// channel: WebM's transparency for VP8 and VP9.
    pub alpha_mode: u64,
    pub colour: Option<Colour>,
}

/// A video track's colour: `Colour`, as far as converting its pictures to
/// RGB needs it, numbered as ITU-T H.273 numbers it. (Its other elements --
/// the transfer function, chroma siting, bits per channel -- are read and
/// checked, but no conversion here obeys them, so they are not kept.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    /// `MatrixCoefficients`: 2, unspecified, unless the file says.
    pub matrix_coefficients: u64,
    /// 0 unspecified, 1 limited, 2 full, 3 defined by the others.
    pub range: u64,
    /// `Primaries`: 2, unspecified, unless the file says.
    pub primaries: u64,
}

/// An audio track's sound: `Audio`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Audio {
    pub sampling_frequency: f64,
    /// The rate to play at: the sampling rate unless the file says
    /// otherwise (spectral band replication).
    pub output_sampling_frequency: f64,
    pub channels: u64,
    pub bit_depth: Option<u64>,
}

/// How a track's frames were transformed before they were stored: the
/// `ContentEncoding` this undoes, or why it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// Stored as written.
    None,
    /// Every frame's leading bytes, cut (`ContentCompAlgo` 3).
    HeaderStripped(Vec<u8>),
    /// zlib (`ContentCompAlgo` 0).
    Zlib,
    /// Something this does not undo -- bzip2, LZO, encryption -- so the
    /// track's frames are not given out.
    Unsupported(&'static str),
}

impl Encoding {
    /// `frame` as it was before the encoding.
    ///
    /// # Errors
    ///
    /// When the frame does not decompress, or the encoding is not undone
    /// here.
    pub(crate) fn undo<'a>(&self, frame: &'a [u8]) -> Result<Cow<'a, [u8]>, Error> {
        match self {
            Self::None => Ok(Cow::Borrowed(frame)),
            Self::HeaderStripped(prefix) => {
                let mut v = Vec::with_capacity(prefix.len().saturating_add(frame.len()));
                v.extend_from_slice(prefix);
                v.extend_from_slice(frame);
                Ok(Cow::Owned(v))
            }
            Self::Zlib => {
                let limit = usize::try_from(MAX_BINARY).unwrap_or(usize::MAX);
                deflate::zlib_inflate_limited(frame, limit)
                    .map(Cow::Owned)
                    .map_err(|_| Error::Invalid("a zlib-compressed frame"))
            }
            Self::Unsupported(why) => Err(Error::Unsupported(why)),
        }
    }
}

/// One track of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// The number blocks name the track by.
    pub number: u64,
    pub uid: u64,
    pub kind: TrackKind,
    /// The codec ID as written, ASCII: `V_VP9`, `A_OPUS`, ...
    pub codec_id: Vec<u8>,
    pub codec: Codec,
    /// The codec's setup -- Vorbis's headers, Opus's `OpusHead`, AV1's
    /// configuration record -- with any encoding that covers it undone.
    pub codec_private: Vec<u8>,
    /// The track's name, as written (UTF-8 by the specification, not
    /// checked).
    pub name: Option<Vec<u8>>,
    /// Its language, ISO 639-2 as written: `eng` unless the file says.
    pub language: Vec<u8>,
    pub enabled: bool,
    pub default: bool,
    pub forced: bool,
    /// How long each frame lasts, in nanoseconds, where they all last the
    /// same.
    pub default_duration: Option<u64>,
    /// How much the codec outputs before its first real sample, in
    /// nanoseconds (Opus's pre-skip). FFmpeg takes it off every timestamp,
    /// and so does this.
    pub codec_delay: u64,
    /// How much sound a decoder needs after a jump before its output is
    /// right, in nanoseconds (Opus: 80 ms). [`crate::Demuxer::seek`] does not
    /// act on it, as FFmpeg's seek does not: a caller seeking in sound seeks
    /// this much before the time it wants, and drops what decodes before it.
    pub seek_pre_roll: u64,
    pub video: Option<Video>,
    pub audio: Option<Audio>,
    /// `TrackTimestampScale`, deprecated: what the Cluster's timestamp is
    /// divided by in this track. 1.0 in every file written today.
    pub(crate) time_scale: f64,
    pub(crate) encoding: Encoding,
}

impl Track {
    /// Whether this crate can give out the track's frames as they were
    /// before their encoding: not for encryption, nor a compression it does
    /// not undo, whose packets [`crate::Demuxer::next_packet`] skips.
    pub fn readable(&self) -> bool {
        !matches!(self.encoding, Encoding::Unsupported(_))
    }
}

/// The fields of one `ContentEncoding`, as FFmpeg reads them: zeroed, so an
/// encoding without a `ContentCompression` is zlib (`ContentCompAlgo` 0).
struct RawEncoding {
    scope: u64,
    kind: u64,
    algo: u64,
    settings: Vec<u8>,
}

/// A `TrackEntry`: its number, and the track -- `None` for one FFmpeg
/// ignores (a kind it does not read, no codec ID, or a codec ID of another
/// kind), whose number still names it.
///
/// # Errors
///
/// When the entry is damaged; or -- each of which FFmpeg refuses the whole
/// file for -- a video track's crop leaves nothing of its picture, or an
/// audio track claims more channels than an `int` holds.
pub(crate) fn read_track<R: Read + Seek>(
    r: &mut Reader<R>,
    entry: &Header,
) -> Result<(u64, Option<Track>), Error> {
    let mut number = 0;
    let mut uid = 0;
    let mut kind = 0;
    let mut codec_id: Option<Vec<u8>> = None;
    let mut codec_private = Vec::new();
    let mut name = None;
    let mut language = None;
    let (mut enabled, mut default, mut forced) = (true, true, false);
    let mut default_duration = 0u64;
    let mut codec_delay = 0;
    let mut seek_pre_roll = 0;
    let mut time_scale = 1.0;
    let mut video: Option<(Video, f64)> = None;
    let mut audio = None;
    let mut encodings: Vec<RawEncoding> = Vec::new();
    r.children(entry, |r, c| {
        match c.id {
            ids::TRACK_NUMBER => number = r.uint(c.size, 0)?,
            ids::TRACK_UID => uid = r.uint(c.size, 0)?,
            ids::TRACK_TYPE => kind = r.uint(c.size, 0)?,
            ids::FLAG_ENABLED => enabled = r.uint(c.size, 1)? != 0,
            ids::FLAG_DEFAULT => default = r.uint(c.size, 1)? != 0,
            ids::FLAG_FORCED => forced = r.uint(c.size, 0)? != 0,
            ids::DEFAULT_DURATION => default_duration = r.uint(c.size, 0)?,
            ids::TRACK_TIMESTAMP_SCALE => time_scale = r.float(c.size, 1.0)?,
            ids::NAME => name = r.string(c.size)?,
            ids::LANGUAGE => language = r.string(c.size)?,
            ids::CODEC_ID => codec_id = r.string(c.size)?,
            ids::CODEC_PRIVATE => codec_private = r.binary(c.size, MAX_BINARY)?,
            ids::CODEC_DELAY => codec_delay = r.uint(c.size, 0)?,
            ids::SEEK_PRE_ROLL => seek_pre_roll = r.uint(c.size, 0)?,
            ids::VIDEO => video = Some(read_video(r, c)?),
            ids::AUDIO => audio = Some(read_audio(r, c)?),
            ids::CONTENT_ENCODINGS => encodings = read_encodings(r, c)?,
            _ => {}
        }
        Ok(())
    })?;

    let Some(kind) = TrackKind::from_type(kind) else {
        return Ok((number, None));
    };
    let Some(codec_id) = codec_id.filter(|id| kind.fits(id)) else {
        return Ok((number, None));
    };

    if kind == TrackKind::Audio
        && audio.is_some_and(|a| a.channels > u64::from(i32::MAX.unsigned_abs()))
    {
        return Err(Error::Unsupported(
            "an audio track of more than 2^31 channels",
        ));
    }
    let audio = audio.filter(|_| kind == TrackKind::Audio).map(|mut a| {
        // FFmpeg: a rate that is not a number, or outside int's range, is
        // 8000; no output rate is the sampling rate.
        if !(0.0..=f64::from(i32::MAX)).contains(&a.sampling_frequency) {
            a.sampling_frequency = 8000.0;
        }
        #[allow(
            clippy::float_cmp,
            reason = "FFmpeg's test is for exactly 0, a rate the file did not give"
        )]
        let unset = a.output_sampling_frequency == 0.0;
        if unset {
            a.output_sampling_frequency = a.sampling_frequency;
        }
        a
    });
    let video = match video.filter(|_| kind == TrackKind::Video) {
        Some((v, frame_rate)) => {
            if default_duration == 0 {
                default_duration = duration_of_frame_rate(frame_rate);
            }
            Some(checked_video(v)?)
        }
        None => None,
    };

    let encoding = encoding_of(&encodings);
    // FFmpeg undoes the codec private data's encoding when the encoding's
    // scope covers it (bit 2), and drops the data if that fails.
    if let [e] = encodings.as_slice()
        && e.scope & 2 != 0
        && matches!(encoding, Encoding::HeaderStripped(_) | Encoding::Zlib)
        && !codec_private.is_empty()
    {
        codec_private = encoding
            .undo(&codec_private)
            .map(Cow::into_owned)
            .unwrap_or_default();
    }
    // And the frames' when it covers them (bit 1).
    let encoding = match encodings.as_slice() {
        [e] if e.scope & 1 != 0 => encoding,
        _ => Encoding::None,
    };

    Ok((
        number,
        Some(Track {
            number,
            uid,
            kind,
            codec: Codec::from_id(&codec_id),
            codec_id,
            codec_private,
            name,
            language: language.unwrap_or_else(|| b"eng".to_vec()),
            enabled,
            default,
            forced,
            default_duration: (default_duration != 0).then_some(default_duration),
            codec_delay,
            seek_pre_roll,
            video,
            audio,
            time_scale,
            encoding,
        }),
    ))
}

/// The default duration FFmpeg makes of the deprecated `FrameRate`: a
/// billion nanoseconds over the rate, truncated, where that is a `u64`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "C's conversion of a double known to be in range to uint64_t, which truncates"
)]
fn duration_of_frame_rate(frame_rate: f64) -> u64 {
    if frame_rate > 0.0 {
        let d = 1e9 / frame_rate;
        if (0.0..=u64::MAX as f64).contains(&d) {
            return d as u64;
        }
    }
    0
}

/// FFmpeg's checks on a video track's crop -- in C's unsigned arithmetic,
/// against `INT_MAX` -- and its defaults for the display size. A track with
/// no size at all passes (FFmpeg takes the size from the codec then); one
/// with a size the crop leaves nothing of does not.
fn checked_video(mut v: Video) -> Result<Video, Error> {
    let int_max = u64::from(i32::MAX.unsigned_abs());
    let [left, top, right, bottom] = v.crop;
    let sizeless = u64::from(v.pixel_width == 0 && v.pixel_height == 0);
    if left >= int_max.wrapping_sub(right)
        || top >= int_max.wrapping_sub(bottom)
        || left.wrapping_add(right) >= v.pixel_width.wrapping_add(sizeless)
        || top.wrapping_add(bottom) >= v.pixel_height.wrapping_add(sizeless)
    {
        return Err(Error::Invalid("a video track cropped to nothing"));
    }
    if v.display_unit == 0 {
        // The checks above keep both differences positive.
        let cropped_width = v.pixel_width.wrapping_sub(left).wrapping_sub(right);
        let cropped_height = v.pixel_height.wrapping_sub(top).wrapping_sub(bottom);
        v.display_width = v.display_width.or(Some(cropped_width));
        v.display_height = v.display_height.or(Some(cropped_height));
    }
    Ok(v)
}

/// FFmpeg's reading of a track's encodings
/// (`matroska_parse_content_encodings`): a single one is undone, several
/// together are passed through as stored; an encryption, or a compression
/// other than header stripping and zlib, leaves the frames unreadable here
/// (FFmpeg passes encrypted frames on for a decrypter, and undoes bzip2 and
/// LZO, which this does not).
fn encoding_of(encodings: &[RawEncoding]) -> Encoding {
    let [e] = encodings else {
        return Encoding::None;
    };
    if e.kind != 0 {
        return Encoding::Unsupported("an encrypted track");
    }
    match e.algo {
        3 if e.settings.is_empty() => Encoding::None,
        3 => Encoding::HeaderStripped(e.settings.clone()),
        0 => Encoding::Zlib,
        1 => Encoding::Unsupported("a bzip2-compressed track"),
        2 => Encoding::Unsupported("an LZO-compressed track"),
        // An algorithm no one knows: FFmpeg passes the frames through.
        _ => Encoding::None,
    }
}

fn read_encodings<R: Read + Seek>(
    r: &mut Reader<R>,
    parent: &Header,
) -> Result<Vec<RawEncoding>, Error> {
    let mut out = Vec::new();
    r.children(parent, |r, child| {
        if child.id != ids::CONTENT_ENCODING {
            return Ok(());
        }
        let mut e = RawEncoding {
            scope: 1,
            kind: 0,
            algo: 0,
            settings: Vec::new(),
        };
        r.children(child, |r, field| {
            match field.id {
                ids::CONTENT_ENCODING_SCOPE => e.scope = r.uint(field.size, 1)?,
                ids::CONTENT_ENCODING_TYPE => e.kind = r.uint(field.size, 0)?,
                ids::CONTENT_COMPRESSION => r.children(field, |r, c| {
                    match c.id {
                        ids::CONTENT_COMP_ALGO => e.algo = r.uint(c.size, 0)?,
                        ids::CONTENT_COMP_SETTINGS => {
                            e.settings = r.binary(c.size, MAX_BINARY)?;
                        }
                        _ => {}
                    }
                    Ok(())
                })?,
                _ => {}
            }
            Ok(())
        })?;
        out.push(e);
        Ok(())
    })?;
    Ok(out)
}

/// `FrameRate`, deprecated in RFC 9559 and still read by FFmpeg.
const FRAME_RATE: Id = 0x23_83E3;

/// A `Video` element, and its deprecated `FrameRate`.
fn read_video<R: Read + Seek>(r: &mut Reader<R>, parent: &Header) -> Result<(Video, f64), Error> {
    let mut v = Video {
        pixel_width: 0,
        pixel_height: 0,
        crop: [0; 4],
        display_width: None,
        display_height: None,
        display_unit: 0,
        alpha_mode: 0,
        colour: None,
    };
    let mut frame_rate = 0.0;
    r.children(parent, |r, c| {
        match c.id {
            ids::PIXEL_WIDTH => v.pixel_width = r.uint(c.size, 0)?,
            ids::PIXEL_HEIGHT => v.pixel_height = r.uint(c.size, 0)?,
            ids::PIXEL_CROP_LEFT => v.crop[0] = r.uint(c.size, 0)?,
            ids::PIXEL_CROP_TOP => v.crop[1] = r.uint(c.size, 0)?,
            ids::PIXEL_CROP_RIGHT => v.crop[2] = r.uint(c.size, 0)?,
            ids::PIXEL_CROP_BOTTOM => v.crop[3] = r.uint(c.size, 0)?,
            ids::DISPLAY_WIDTH => v.display_width = Some(r.uint(c.size, 0)?),
            ids::DISPLAY_HEIGHT => v.display_height = Some(r.uint(c.size, 0)?),
            ids::DISPLAY_UNIT => v.display_unit = r.uint(c.size, 0)?,
            // Read as FFmpeg reads them -- so that a malformed one refuses the
            // track as it does there -- and kept by nothing, because nothing
            // here acts on them (design-decisions §856): no deinterlacing, no
            // stereoscopic display.
            ids::FLAG_INTERLACED | ids::STEREO_MODE => {
                r.uint(c.size, 0)?;
            }
            ids::ALPHA_MODE => v.alpha_mode = r.uint(c.size, 0)?,
            ids::COLOUR => v.colour = Some(read_colour(r, c)?),
            FRAME_RATE => frame_rate = r.float(c.size, 0.0)?,
            _ => {}
        }
        Ok(())
    })?;
    Ok((v, frame_rate))
}

fn read_colour<R: Read + Seek>(r: &mut Reader<R>, parent: &Header) -> Result<Colour, Error> {
    // H.273's "unspecified" (2) for the two code points; 0 for the range.
    let mut c = Colour {
        matrix_coefficients: 2,
        range: 0,
        primaries: 2,
    };
    r.children(parent, |r, e| {
        match e.id {
            ids::MATRIX_COEFFICIENTS => c.matrix_coefficients = r.uint(e.size, 2)?,
            ids::RANGE => c.range = r.uint(e.size, 0)?,
            ids::PRIMARIES => c.primaries = r.uint(e.size, 2)?,
            // Read as FFmpeg reads them, and kept by nothing: the conversion to
            // RGB takes no account of chroma siting or of the transfer
            // function (no colour management, no tone mapping), and the
            // decoder says the depth and subsampling itself.
            ids::BITS_PER_CHANNEL
            | ids::CHROMA_SUBSAMPLING_HORZ
            | ids::CHROMA_SUBSAMPLING_VERT
            | ids::CHROMA_SITING_HORZ
            | ids::CHROMA_SITING_VERT
            | ids::TRANSFER_CHARACTERISTICS => {
                r.uint(e.size, 0)?;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(c)
}

fn read_audio<R: Read + Seek>(r: &mut Reader<R>, parent: &Header) -> Result<Audio, Error> {
    let mut a = Audio {
        sampling_frequency: 8000.0,
        output_sampling_frequency: 0.0,
        channels: 1,
        bit_depth: None,
    };
    r.children(parent, |r, c| {
        match c.id {
            ids::SAMPLING_FREQUENCY => a.sampling_frequency = r.float(c.size, 8000.0)?,
            ids::OUTPUT_SAMPLING_FREQUENCY => a.output_sampling_frequency = r.float(c.size, 0.0)?,
            ids::CHANNELS => a.channels = r.uint(c.size, 1)?,
            ids::BIT_DEPTH => a.bit_depth = Some(r.uint(c.size, 0)?),
            _ => {}
        }
        Ok(())
    })?;
    Ok(a)
}
