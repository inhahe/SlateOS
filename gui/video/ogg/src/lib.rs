//! Ogg files, taken apart into their streams' packets.
//!
//! Ogg (RFC 3533) is Xiph's container: `.ogg` and `.oga` files of Vorbis
//! (or FLAC, or Speex) sound, `.opus` files of Opus (RFC 7845), `.ogv` of
//! Theora pictures with Vorbis sound. A file is a run of pages, each
//! belonging to one logical stream by its serial number and carrying the
//! segments its stream's packets are cut into; a stream's first pages hold
//! its codec's headers, the rest its data, and each page's granule position
//! says how far into the stream the packets ending on it reach. Streams can
//! run side by side (a film's pictures and sound), and one after another --
//! a *chained* file, as an internet radio stream recorded from one song
//! into the next writes it, each song a link of its own.
//!
//! [`Demuxer`] reads one: its streams and their headers when opened
//! ([`Stream`]), then every data packet in file order
//! ([`Demuxer::next_packet`]), each timed in its stream's ticks, and from a
//! time on ([`Demuxer::seek`]), found by bisection over the pages' granule
//! positions.
//!
//! # Whose rules
//!
//! RFC 3533 says what a page is and RFC 7845 and the Vorbis I
//! specification what Opus's and Vorbis's granule positions mean, and this
//! is written from them. Where a reader has a choice -- when a packet
//! starts, what of the last packet is cut off, what a stream's first page
//! says of where it starts -- this makes FFmpeg's (its Ogg demuxer as of
//! 2026-03, git `9b7439c31b`, the reader behind Chrome's Ogg playback, VLC
//! and mpv), and every fixture's packets are held to `ffprobe`'s.
//! FFmpeg's behaviour, not its code: nothing here is translated from
//! FFmpeg, which is LGPL. Where FFmpeg is wrong -- a Vorbis packet it
//! mistimes, a lost page whose packet's ends it joins, a chained file whose
//! times start again at each link -- this does what the stream means, and
//! `stream.rs` and the demuxer's docs say how it differs.
//!
//! # A hostile file
//!
//! Errors, never a panic. A page is taken only whole and with its CRC
//! right; anything else is passed over to the next page. A stream's packet
//! is at most what the file holds; a packet cut by a lost page is lost, not
//! joined to whatever comes next. Nothing allocated is bigger than the
//! pages read.

mod demux;
mod page;
mod stream;

pub use demux::Demuxer;
pub use stream::{Codec, Stream};

use std::fmt;

/// A packet of a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The stream's place in [`Demuxer::streams`].
    pub stream: usize,
    /// When the first sample it decodes to plays, in the stream's ticks
    /// ([`Stream::rate`] a second); `None` where nothing in the file says
    /// (FFmpeg's demuxer leaves it unset there too: a packet after the
    /// first on a stream's last page, a packet of a stream not timed here).
    pub pts: Option<i64>,
    /// How long it lasts, in ticks, its discard padding taken off; 0 where
    /// it cannot be told.
    pub duration: u64,
    pub data: Vec<u8>,
    /// Samples to drop from the start of what it decodes to: Opus's
    /// pre-skip, with the stream's first packet (and with its first after a
    /// seek to its start, or into a link of a chained file).
    pub skip_samples: u32,
    /// Samples to drop from the end of what it decodes to: what the stream's
    /// last page says runs past its end.
    pub discard_padding: u32,
    /// Its length could not be read from its first bytes: damaged.
    pub corrupt: bool,
    /// Where its bytes begin in the file.
    pub position: u64,
    /// Where the page it begins on begins: what FFmpeg gives as a packet's
    /// position.
    pub page_position: u64,
    /// The stream's headers, where they change from here on: the next link's
    /// in a chained file, and the link's after a seek into a chained file.
    pub new_headers: Option<Vec<Vec<u8>>>,
}

/// Why a file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed.
    Io(std::io::ErrorKind),
    /// The file breaks the format's rules: the words say which.
    Invalid(&'static str),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.kind())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "the Ogg file cannot be read: {kind}"),
            Self::Invalid(why) => write!(f, "not an Ogg file this can read: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Whether `head` -- a file's first bytes -- is an Ogg file's start, as
/// FFmpeg's probe asks: the capture pattern, version 0, and flags no page
/// can have past the three defined.
pub fn probe(head: &[u8]) -> bool {
    head.len() >= 6 && head.starts_with(b"OggS\0") && head.get(5).is_some_and(|&f| f <= 7)
}
