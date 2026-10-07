//! Matroska and WebM files, taken apart into their tracks' packets.
//!
//! WebM -- the format of most video on the web that is not MP4 -- is
//! Matroska with fewer choices: VP8, VP9 or AV1 video, Vorbis or Opus sound,
//! WebVTT subtitles. A file is a tree of EBML elements (`ebml`): a header
//! saying which of the two it is, then one Segment holding the tracks'
//! descriptions (`Tracks`), the packets in Clusters, each a run of blocks
//! with timestamps relative to its own, and usually an index of where the
//! key frames are (`Cues`).
//!
//! [`Demuxer`] reads one: the segment's description and its tracks
//! ([`Track`]) when opened, then each packet in file order
//! ([`Demuxer::next_packet`]), and from a time on ([`Demuxer::seek`]) --
//! through the Cues where the file has them for the track, and where it
//! does not by walking the Clusters for key frames, only as far as the time
//! sought. A packet's timestamp is FFmpeg's: the Cluster's plus the
//! block's, less the track's codec delay, in the segment's ticks
//! (each [`Demuxer::time_base`] seconds; the segment's `TimestampScale`
//! nanoseconds in every file written today).
//!
//! What a file says of itself comes as FFmpeg gives it (the [`metadata`]
//! module has its rules): the file's metadata ([`Demuxer::metadata`]: its
//! title, tags), each track's ([`Track::metadata`]: language, name, tags),
//! its chapters ([`Demuxer::chapters`], [`Demuxer::chapter_ends`]) and its
//! attachments -- cover art, fonts -- whose bytes are read when asked for
//! ([`Demuxer::attachments`], [`Demuxer::attachment_data`]).
//!
//! # Whose rules
//!
//! RFC 9559 (Matroska) and RFC 8794 (EBML) say what a file means, and this
//! is written from them. Where a reader has a choice -- how to time a laced
//! block's frames, when an empty frame is a packet, what a codec delay does
//! to a timestamp, which damage ends what -- this makes FFmpeg's choice (its
//! `libavformat/matroskadec.c` as of 2026-03, git `9b7439c31b`, the
//! demuxer behind Chrome, VLC and
//! mpv), and every fixture's packets are held to `ffprobe`'s. FFmpeg's
//! behaviour, not its code: nothing here is translated from FFmpeg, which is
//! LGPL.
//!
//! # A hostile file
//!
//! Errors, never a panic. Every element is bounded by its parent and by the
//! file's length before anything is read or allocated for it; a block holds
//! at most 256 laces and a binary element at most 256 MiB (FFmpeg's limit).
//! A damaged Cluster ends reading at that point, and the next packet comes
//! from the next top-level element the file still holds, found by its ID as
//! FFmpeg resynchronises -- the search starting one byte past the last
//! element FFmpeg would have counted good, so that even a frame whose bytes
//! look like a Cluster is found or missed as FFmpeg finds or misses it.
//!
//! # Held to FFmpeg
//!
//! Every packet of 48 files, and the packets after 97 seeks, are ffprobe's
//! (`tests/fixtures.rs`, `tests/data/generate_fixtures.py`), and so is the
//! display matrix -- or the refusal -- of 22 files' video projections
//! ([`Video::display_matrix`], `tests/projection.rs`), and the metadata,
//! chapters and attachments of eleven files and of one cut short at each of
//! 660 lengths (`tests/metadata.rs`); what ffprobe cannot show is in
//! `tests/beyond_ffprobe.rs` and `tests/metadata.rs`; and `mutate.py` breaks
//! the code one rule at a time and checks the tests notice.

mod block;
mod cues;
mod demux;
mod ebml;
mod ids;
pub mod metadata;
mod nest;
mod track;

pub use demux::{Demuxer, Packet, SegmentInfo};
pub use metadata::{Attachment, AttachmentKind, Chapter, Metadata};
pub use track::{Audio, Codec, Colour, Projection, Track, TrackKind, Video};

/// Why a file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed.
    Io(std::io::ErrorKind),
    /// The file ends inside an element.
    Truncated,
    /// The file breaks the format's rules: the words say which.
    Invalid(&'static str),
    /// The file uses something this does not read.
    Unsupported(&'static str),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::UnexpectedEof => Self::Truncated,
            kind => Self::Io(kind),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "cannot read the Matroska file: {kind}"),
            Self::Truncated => f.write_str("the Matroska file ends too soon"),
            Self::Invalid(why) => write!(f, "damaged Matroska file: {why}"),
            Self::Unsupported(why) => write!(f, "unsupported Matroska file: {why}"),
        }
    }
}

impl std::error::Error for Error {}
