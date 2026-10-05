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
//! out are decoded, for those after them, and not shown. A picture the file
//! asks to be turned or mirrored -- MP4's display matrix, Matroska's
//! projection -- comes out turned, as ffmpeg's autorotate turns it
//! ([`Orientation`]). Not yet: H.264 and
//! HEVC, which most MP4 files hold (`roadmap.md`, "Video files"). An Ogg
//! film's pictures are Theora, refused by name; its sound plays ([`Sound`]).
//!
//! # Sound
//!
//! [`Sound`] is a file's sound, as [`Video`] is its pictures: opened on the
//! same file (a second handle to it), it gives back each packet's samples
//! decoded, with their time on the same clock, for the program to play and
//! to show the pictures by. Opus and Vorbis, in Matroska and WebM -- WebM's
//! sound --, Opus in MP4, and Ogg files' (`.opus`, `.ogg`, `.oga`, and an
//! `.ogv` film's sound; chained files played as one); FLAC, in `.flac` files
//! and in Ogg, Matroska and MP4 -- through `gui/video/opus`, libopus's
//! decoder, `gui/video/vorbis`, Tremor, and `gui/video/flac`, libFLAC, each
//! ported and held to its reference sample for sample; the codec delay, an
//! MP4 edit list's priming and each packet's discard padding dropped, and
//! the blocks timed, as FFmpeg drops and times them (`tests/sound.rs`).
//! A file whose sound is AAC or MP3 is refused by the codec's name.
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
mod orientation;
mod picture;
mod sound;
mod time;
mod video;

pub use colour::Colour;
pub use decoder::{Decoder, Packet};
pub use orientation::Orientation;
pub use picture::Picture;
pub use sound::{Block, Sound, SoundInfo};
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
    /// Theora, Ogg's video codec. Not decoded here.
    Theora,
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
            Self::Theora => "Theora",
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
    /// An Ogg file that could not be read.
    Ogg(ogg::Error),
    /// A native FLAC file that could not be read.
    Flac(flac::Error),
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("the file is not a Matroska, WebM, MP4, Ogg or FLAC file"),
            Self::Io(kind) => write!(f, "the file cannot be read: {kind}"),
            Self::Matroska(e) => write!(f, "{e}"),
            Self::Mp4(e) => write!(f, "{e}"),
            Self::Ogg(e) => write!(f, "{e}"),
            Self::Flac(e) => write!(f, "{e}"),
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

impl From<ogg::Error> for ContainerError {
    fn from(e: ogg::Error) -> Self {
        Self::Ogg(e)
    }
}

impl From<flac::Error> for ContainerError {
    fn from(e: flac::Error) -> Self {
        Self::Flac(e)
    }
}

/// A sound codec: those decoded here, and the commonest of the rest, so that
/// a file in one is refused by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundCodec {
    Opus,
    Vorbis,
    /// Not decoded here yet.
    Aac,
    Flac,
    /// Not decoded here yet.
    Mp3,
    /// Any other.
    Other,
}

impl fmt::Display for SoundCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
            Self::Aac => "AAC",
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
            Self::Other => "a codec this does not know",
        })
    }
}

/// Why a file, a packet or a picture could not be read. More reasons may
/// come, as more is decoded: match the ones that matter, and the rest as a
/// whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The file could not be read: the source failed, the file is not
    /// Matroska (WebM), MP4 or Ogg, or its headers are damaged.
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
    /// The file has no sound track, or not the one asked for.
    NoSound,
    /// The sound is in a codec this does not decode.
    SoundCodec(SoundCodec),
    /// An Opus track's setup (its `OpusHead`) is not one, or a packet did not
    /// decode.
    Opus(opus::Error),
    /// A Vorbis track's headers (its codec private data) are not Vorbis's,
    /// or a packet did not decode.
    Vorbis(vorbis::Error),
    /// A FLAC track's setup is not a FLAC stream's description, or a frame
    /// did not decode (as libFLAC would report it).
    Flac(flac::Status),
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
            Self::NoSound => f.write_str("the file has no sound that can be played"),
            Self::SoundCodec(SoundCodec::Other) => {
                f.write_str("the sound's codec is not one decoded here")
            }
            Self::SoundCodec(c) => write!(f, "the sound is {c}, which is not decoded here yet"),
            Self::Opus(e) => write!(f, "the sound could not be decoded: {e}"),
            Self::Vorbis(e) => write!(f, "the sound could not be decoded: {e}"),
            Self::Flac(e) => write!(f, "the sound could not be decoded: {e}"),
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

impl From<ogg::Error> for Error {
    fn from(e: ogg::Error) -> Self {
        Self::Container(ContainerError::Ogg(e))
    }
}

impl From<flac::Error> for Error {
    fn from(e: flac::Error) -> Self {
        Self::Container(ContainerError::Flac(e))
    }
}
