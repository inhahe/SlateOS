//! Video files, for the programs that show them: a file's pictures, in the
//! order they are shown, each with its time, decoded and turned into
//! `0xAARRGGBB` pixels by the stream's own colour.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut video = videocodec::Video::open(std::fs::File::open("clip.webm")?)?;
//! while let Some(frame) = video.next_frame()? {
//!     // Show `frame.pixels` (`frame.width` x `frame.height`) at `frame.time`
//!     // nanoseconds into the file.
//!     # let _ = frame;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Video`] is the one thing a player calls: it reads the file
//! (`gui/video/matroska` or `gui/video/mp4`, by what the file turns out to
//! be), picks its video track, decodes each packet ([`Decoder`]), and
//! converts each picture ([`Picture::to_frame`]). Seeking is
//! [`Video::seek`], to the exact frame or to the key frame before it.
//! [`Decoder`] is the layer beneath, for a program whose packets come from
//! somewhere else.
//!
//! # What plays
//!
//! Matroska, WebM and MP4 files, told apart by their first bytes as FFmpeg
//! tells them; VP8, and VP9 in every profile (8-, 10- and 12-bit; 4:2:0,
//! 4:2:2, 4:4:0, 4:4:4; RGB), each with WebM's alpha channel; and AV1. An
//! MP4 file's edit list is obeyed as FFmpeg obeys it: the frames it leaves
//! out are decoded, for those after them, and not shown. Not yet: H.264 and
//! HEVC, which most MP4 files hold, and sound (`roadmap.md`, "Video
//! files").
//!
//! # Colour
//!
//! Each picture is converted by its own colour description -- its matrix,
//! primaries and range, from its bitstream, else from the file, else guessed
//! from its size as players guess it (`colour.rs`) -- through
//! `gui/video/yuv`'s port of libavif's conversion, so a frame has exactly the
//! pixels an AVIF still of the same picture would: libyuv's fixed point for
//! BT.601, BT.709 and BT.2020, chroma upsampled bilinearly; libavif's
//! floating point for the rest. Held to libavif itself, frame by frame
//! (`tests/frames.rs`).
//!
//! # Time
//!
//! In nanoseconds, on the file's own clock: a frame's [`Frame::time`] is
//! when it is shown, counted from the file's zero (usually its start; a
//! frame before it is negative). Its [`Frame::duration`] is how long, when
//! the file says; when it does not, the frame lasts until the next one's
//! time.
//!
//! # A hostile file
//!
//! Errors, never a panic. A packet that fails to decode costs its own
//! picture, and what follows is decoded as each codec's reference decoder
//! decodes it: VP9 from its next key frame (libvpx's resync), VP8 on from
//! what its references hold (as libvpx shows it, and as FFmpeg's decoder
//! does), AV1 as dav1d does. [`Video::next_frame`] reads on past it; only
//! the source failing ends the reading. Pictures larger than
//! [`Limits::max_pixels`] are refused before anything is allocated for them.

mod colour;
mod container;
mod decoder;
mod picture;
mod time;
mod video;

pub use colour::Colour;
pub use decoder::{Decoder, Packet};
pub use picture::Picture;
pub use video::{SeekMode, Video, VideoInfo};

use core::fmt;

/// A video codec: those decoded here, and the commonest of the rest, so
/// that a file in one is refused by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Vp8,
    Vp9,
    Av1,
    /// Not decoded here yet.
    H264,
    /// Not decoded here yet.
    Hevc,
    /// MPEG-4 Part 2 (DivX, Xvid). Not decoded here.
    Mpeg4,
    /// Any other.
    Other,
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Vp8 => "VP8",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Mpeg4 => "MPEG-4 Part 2",
            Self::Other => "a codec this does not know",
        })
    }
}

/// What a file says of a track's colour, for whatever the bitstream leaves
/// unsaid: each `None` where the file is silent. H.273's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColourHint {
    pub matrix: Option<u16>,
    pub primaries: Option<u16>,
    pub full_range: Option<bool>,
}

/// What a decoder will take on before it refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most pixels a picture may have. A stream asking for more is
    /// refused from its header, before a frame is allocated.
    ///
    /// Defaults to VP9's level 6.2 (8192 x 4352): every size video is made
    /// at, 8K included.
    pub max_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pixels: vp9::DEFAULT_MAX_PIXELS,
        }
    }
}

/// One picture, ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// When it is shown, in nanoseconds on the file's clock.
    pub time: i64,
    /// How long it is shown, in nanoseconds, when the file says: its
    /// block's duration, or the track's frame duration. 0 when it does not,
    /// and then it lasts until the next frame's time.
    pub duration: u64,
    /// Whether it is a key frame: one decoding can start from.
    pub keyframe: bool,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, row by row, `0xAARRGGBB` with straight
    /// alpha: `imagecodec::Image`'s form, ready for `upload_image` with
    /// `BufferFormat::Argb8888`.
    pub pixels: Vec<u32>,
}

/// Why a file's container could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerError {
    /// Neither a Matroska (or WebM) file nor an MP4 one.
    Unknown,
    /// The source failed before the file's kind was known.
    Io(std::io::ErrorKind),
    /// A Matroska or WebM file that could not be read.
    Matroska(matroska::Error),
    /// An MP4 file that could not be read.
    Mp4(mp4::Error),
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("the file is not a Matroska, WebM or MP4 file"),
            Self::Io(kind) => write!(f, "the file cannot be read: {kind}"),
            Self::Matroska(e) => write!(f, "{e}"),
            Self::Mp4(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ContainerError {}

impl From<matroska::Error> for ContainerError {
    fn from(e: matroska::Error) -> Self {
        Self::Matroska(e)
    }
}

impl From<mp4::Error> for ContainerError {
    fn from(e: mp4::Error) -> Self {
        Self::Mp4(e)
    }
}

/// Why a file, a packet or a picture could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file could not be read: the source failed, the file is neither
    /// Matroska (WebM) nor MP4, or its headers are damaged.
    Container(ContainerError),
    /// The file has no video track, or not the one asked for.
    NoVideo,
    /// The video is in a codec this does not decode.
    Codec(Codec),
    /// A VP8 packet did not decode.
    Vp8(vp8::Error),
    /// A VP9 packet did not decode.
    Vp9(vp9::Error),
    /// An AV1 packet did not decode.
    Av1(rav1d::safe::Error),
    /// A picture whose colour cannot be converted: a matrix libavif refuses
    /// (BT.2020 constant luminance, ICtCp, ...), or the identity matrix with
    /// subsampled chroma.
    Colour(yuv::reformat::Error),
    /// A picture larger than [`Limits::max_pixels`].
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(e) => write!(f, "the video file could not be read: {e}"),
            Self::NoVideo => f.write_str("the file has no video that can be played"),
            Self::Codec(Codec::Other) => f.write_str("the video's codec is not one decoded here"),
            Self::Codec(c) => write!(f, "the video is {c}, which is not decoded here yet"),
            Self::Vp8(e) => write!(f, "{e}"),
            Self::Vp9(e) => write!(f, "{e}"),
            Self::Av1(e) => write!(f, "{e}"),
            Self::Colour(yuv::reformat::Error::Unsupported) => {
                f.write_str("the video's colour description cannot be converted to pixels")
            }
            Self::Colour(yuv::reformat::Error::Size) => {
                f.write_str("a decoded picture's planes do not match its size")
            }
            Self::TooLarge => f.write_str("the video's pictures are larger than allowed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ContainerError> for Error {
    fn from(e: ContainerError) -> Self {
        Self::Container(e)
    }
}

impl From<matroska::Error> for Error {
    fn from(e: matroska::Error) -> Self {
        Self::Container(ContainerError::Matroska(e))
    }
}

impl From<mp4::Error> for Error {
    fn from(e: mp4::Error) -> Self {
        Self::Container(ContainerError::Mp4(e))
    }
}
