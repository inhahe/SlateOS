//! MP4 files -- the ISO base media file format, as `.mp4`, `.m4a`, `.m4v`
//! and their fragmented kin for streaming are written -- taken apart into
//! their tracks' packets.
//!
//! An MP4 file is a sequence of *boxes*: a size, a four-letter type, and the
//! body. The media itself sits in `mdat` boxes, undivided; what divides it
//! is the `moov` box, before or after the media, which holds each track's
//! description (`trak`) and its *sample tables*: where each chunk of samples
//! begins, how many samples each chunk holds, each sample's size, how long
//! each lasts, which may start decoding, and how far each is shown after it
//! is decoded. An *edit list* then says which stretch of each track plays,
//! and from when. A fragmented file carries the tables in pieces instead,
//! one `moof` box before each run of media.
//!
//! [`Demuxer`] reads one: the tracks ([`Track`]) when opened -- every
//! table, and every fragment's -- then each packet in the order FFmpeg
//! gives them ([`Demuxer::next_packet`]), and from a time on
//! ([`Demuxer::seek`]).
//!
//! # Whose rules
//!
//! ISO/IEC 14496-12 says what a file means, and FFmpeg's demuxer
//! (`libavformat/mov.c` as of 2026-03, git `9b7439c31b`, the demuxer behind
//! Chrome, VLC and mpv) decides everything it leaves open: how an edit list
//! trims and shifts a track, which packets are kept only so that others can
//! be decoded (marked [`Packet::discard`]), what a negative composition
//! offset does to the decoding times, how the tracks' packets interleave,
//! and how damage is read. Every fixture's packets and seeks are held to
//! `ffprobe`'s.
//!
//! # Whose code
//!
//! FFmpeg's, translated: the index building, the edit lists, the fragments
//! and the reading order follow `mov.c`'s functions closely, each named
//! where it is ported. FFmpeg is LGPL version 2.1 or later, and so is this
//! crate, its licence in `licenses/`. Whether SlateOS keeps an LGPL demuxer
//! or has one written again from the specification and these tests is
//! `open-questions/F-Q7.md`.
//!
//! # A hostile file
//!
//! Errors, never a panic. Every table is bounded by the file's length
//! before anything is allocated for it, and every box by its parent.

mod demux;
mod index;
mod parse;
mod reader;
mod track;

pub use demux::{Demuxer, Packet};
pub use track::{Audio, Codec, Colour, Track, TrackKind, Video};

/// Why a file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed.
    Io(std::io::ErrorKind),
    /// The file ends inside something it promised.
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
            Self::Io(kind) => write!(f, "cannot read the MP4 file: {kind}"),
            Self::Truncated => f.write_str("the MP4 file ends too soon"),
            Self::Invalid(why) => write!(f, "damaged MP4 file: {why}"),
            Self::Unsupported(why) => write!(f, "unsupported MP4 file: {why}"),
        }
    }
}

impl std::error::Error for Error {}
