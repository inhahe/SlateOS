//! What a video file says about itself -- its container, how long it plays,
//! and each track's codec, picture size, frame rate, sample rate, channels,
//! language and name -- read from its headers, without decoding a frame.
//!
//! `apps/videoplayer` carried every type this fills -- a container, streams,
//! codecs, a length -- and nothing that read a file into them: it had no way
//! to open one, and its window said so. This is `apps/audiotags`' counterpart
//! for moving pictures, the half of "open a video" that needs no decoder.
//!
//! # What is read, and from where
//!
//! | Container | Length | Tracks |
//! |---|---|---|
//! | MP4, M4V, MOV, 3GP | `mvhd` (`mehd` for a fragmented file) | each `trak`: `tkhd`, `mdhd`, `hdlr`, the first `stsd` entry, `stsz`'s count |
//! | Matroska, WebM | `Info`'s `Duration` at its `TimestampScale` | each `TrackEntry` |
//! | AVI | the video stream's `strh` (OpenDML's `dmlh` count past 1 GiB) | each `strl`: `strh`, `strf`, `strn` |
//!
//! Everything is bounded: boxes and elements are stepped over by their sizes
//! with a seek, only the small ones this reads are read, each to a cap, and a
//! size that would run past its parent or the file ends the walk rather than
//! a panic or a loop.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

mod avi;
mod mkv;
mod mp4;
pub mod testing;

/// The most children one box or element is walked through. A real file has
/// tens; a crafted one of zero-length boxes would otherwise be walked for as
/// long as it is.
const MAX_CHILDREN: usize = 4096;

/// A video file's container, by what its first bytes say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Container {
    /// ISO base media: MP4, M4V, 3GP.
    Mp4,
    /// Apple's original of the same box structure: MOV.
    QuickTime,
    Matroska,
    /// Matroska restricted to open codecs, and named so in its header.
    WebM,
    Avi,
    #[default]
    Unknown,
}

impl Container {
    /// The container the first bytes of a file name. Not the file's name:
    /// that is a claim by whoever gave it, and the content is the fact.
    #[must_use]
    pub fn detect(head: &[u8]) -> Self {
        if head.get(..4) == Some(&[0x1A, 0x45, 0xDF, 0xA3]) {
            mkv::container(head)
        } else if head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"AVI ") {
            Self::Avi
        } else {
            mp4::container(head)
        }
    }

    /// The container's usual name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Mp4 => "MPEG-4",
            Self::QuickTime => "QuickTime",
            Self::Matroska => "Matroska",
            Self::WebM => "WebM",
            Self::Avi => "AVI",
            Self::Unknown => "Unknown",
        }
    }
}

/// What a track carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
    Subtitle,
    /// A timecode, a hint track, a chapter list: nothing to watch or hear.
    #[default]
    Other,
}

/// How a track is encoded, as far as a header names it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    Av1,
    Vp8,
    Vp9,
    Mpeg4Visual,
    Mpeg2Video,
    Mpeg1Video,
    MotionJpeg,
    Theora,
    Vc1,
    Aac,
    Mp3,
    Mp2,
    Opus,
    Vorbis,
    Flac,
    Alac,
    Pcm,
    Ac3,
    Eac3,
    Dts,
    TrueHd,
    Wma,
    /// Timed text: MP4's `tx3g`, and plain UTF-8 subtitles (SRT) in Matroska.
    Text,
    WebVtt,
    Ttml,
    Ass,
    Ssa,
    Pgs,
    VobSub,
    DvbSub,
    /// A codec this does not name, as the file does: a four-letter code or a
    /// Matroska codec id.
    Other(String),
    /// The file names no codec.
    #[default]
    Unknown,
}

impl Codec {
    /// The codec's usual name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
            Self::Av1 => "AV1",
            Self::Vp8 => "VP8",
            Self::Vp9 => "VP9",
            Self::Mpeg4Visual => "MPEG-4 Visual",
            Self::Mpeg2Video => "MPEG-2",
            Self::Mpeg1Video => "MPEG-1",
            Self::MotionJpeg => "Motion JPEG",
            Self::Theora => "Theora",
            Self::Vc1 => "VC-1",
            Self::Aac => "AAC",
            Self::Mp3 => "MP3",
            Self::Mp2 => "MP2",
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Pcm => "PCM",
            Self::Ac3 => "AC-3",
            Self::Eac3 => "E-AC-3",
            Self::Dts => "DTS",
            Self::TrueHd => "TrueHD",
            Self::Wma => "WMA",
            Self::Text => "Text",
            Self::WebVtt => "WebVTT",
            Self::Ttml => "TTML",
            Self::Ass => "ASS",
            Self::Ssa => "SSA",
            Self::Pgs => "PGS",
            Self::VobSub => "VobSub",
            Self::DvbSub => "DVB",
            Self::Other(name) => name,
            Self::Unknown => "Unknown",
        }
    }
}

/// One track: each part `None` where the file does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub kind: Kind,
    pub codec: Codec,
    /// The picture's size in pixels, as stored.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Frames a second: the average, for a track whose rate varies.
    pub frame_rate: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    /// The language as the file gives it: an ISO 639-2 code (`eng`) or a BCP
    /// 47 tag (`en-GB`). `None` for "undetermined".
    pub language: Option<String>,
    /// The track's own name, where the file gives one: "Commentary".
    pub name: Option<String>,
    /// The track a player picks when nothing else is asked for.
    pub default: bool,
    /// Shown whatever the viewer chose: a subtitle for speech in another
    /// language than the rest.
    pub forced: bool,
}

/// What a video file says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub container: Container,
    pub duration_secs: Option<f64>,
    /// In the file's order.
    pub tracks: Vec<Track>,
    /// The title the file carries, not its name.
    pub title: Option<String>,
}

impl Probe {
    /// The first track of `kind`.
    #[must_use]
    pub fn first(&self, kind: Kind) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == kind)
    }

    /// The track of `kind` a player would pick: the first marked default,
    /// else the first.
    #[must_use]
    pub fn preferred(&self, kind: Kind) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.kind == kind && t.default)
            .or_else(|| self.first(kind))
    }
}

/// Read the video file at `path`.
///
/// # Errors
///
/// Only when the file cannot be opened or read; a file that is not a video
/// this reads is a [`Probe`] of [`Container::Unknown`] with nothing in it.
pub fn probe_path(path: &Path) -> io::Result<Probe> {
    let mut file = std::fs::File::open(path)?;
    probe(&mut file)
}

/// [`probe_path`], from anything that can be read and sought in.
///
/// # Errors
///
/// When a read or a seek fails.
pub fn probe<R: Read + Seek>(r: &mut R) -> io::Result<Probe> {
    let len = r.seek(SeekFrom::End(0))?;
    // Enough for Matroska's EBML header, whose document type says WebM.
    let head = read_at(r, 0, 4096, len)?;
    let container = Container::detect(&head);
    let mut probe = match container {
        Container::Mp4 | Container::QuickTime => mp4::probe(r, len)?,
        Container::Matroska | Container::WebM => mkv::probe(r, len)?,
        Container::Avi => avi::probe(r, len)?,
        Container::Unknown => Probe::default(),
    };
    probe.container = container;
    // A length that is not a length -- a header's zero, or a NaN in a float
    // field -- says nothing.
    probe.duration_secs = probe.duration_secs.filter(|s| s.is_finite() && *s > 0.0);
    for track in &mut probe.tracks {
        track.frame_rate = track.frame_rate.filter(|f| f.is_finite() && *f > 0.0);
    }
    Ok(probe)
}

/// Up to `want` bytes from `at`, fewer at the end of the file.
fn read_at<R: Read + Seek>(r: &mut R, at: u64, want: usize, len: u64) -> io::Result<Vec<u8>> {
    let room = usize::try_from(len.saturating_sub(at)).unwrap_or(usize::MAX);
    let mut buf = vec![0; want.min(room)];
    if buf.is_empty() {
        return Ok(buf);
    }
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// `N` bytes at `at` in `b`.
fn bytes<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    bytes(b, at).map(u16::from_be_bytes)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    bytes(b, at).map(u32::from_be_bytes)
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    bytes(b, at).map(u64::from_be_bytes)
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    bytes(b, at).map(u16::from_le_bytes)
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    bytes(b, at).map(u32::from_le_bytes)
}

/// `count` units at `rate` a second, in seconds; `None` for no rate.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of ticks and a rate are far inside f64's integer-exact range"
)]
fn seconds(count: u64, rate: u64) -> Option<f64> {
    (rate > 0).then(|| count as f64 / rate as f64)
}

/// Text a header holds: UTF-8, trimmed of the NULs a fixed field is padded
/// with and of spaces; `None` if nothing is left or it is not UTF-8.
fn text(b: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(b).ok()?;
    let t = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!t.is_empty()).then(|| {
        t.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    })
}

/// A language code as a header gives it, `None` for "undetermined".
fn language(code: &str) -> Option<String> {
    let c = code.trim();
    (!c.is_empty() && !c.eq_ignore_ascii_case("und")).then(|| c.to_owned())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::float_cmp
)]
mod tests;
