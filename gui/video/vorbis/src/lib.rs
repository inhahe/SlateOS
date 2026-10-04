//! Vorbis: Tremor, Xiph's integer Vorbis decoder, ported to Rust and held
//! to it sample for sample.
//!
//! A [`Decoder`] is made from a stream's identification and setup headers
//! and then given its audio packets in order; each returns the samples it
//! completes (a packet's first half overlaps the last one's second, so the
//! first packet, and the first after [`Decoder::reset`], completes none).
//! [`Comments::parse`] reads the comment header's tags. Packets come from
//! the stream's container: Ogg pages, or Matroska blocks with the headers
//! in the track's private data.
//!
//! Translated from Tremor (the integer decoder in Xiph's `tremor`
//! repository), copyright Xiph.Org, used under its BSD licence
//! (`licenses/tremor-COPYING`); each module names the files it came from.

#![forbid(unsafe_code)]

mod bitpack;
mod codebook;
mod decoder;
mod floor0;
mod floor1;
mod info;
mod mapping;
mod mdct;
mod misc;
mod residue;
mod tables;
mod window;

use std::fmt;

pub use decoder::Decoder;
pub use info::{Comments, Info};

/// Why a header could not be read, or a packet decoded: Tremor's error
/// codes, as a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// The output buffer is too small for the packet's samples (Tremor has
    /// no such case; `OV_EINVAL`).
    BufferTooSmall,
    /// A header that is not a Vorbis header at all (`OV_ENOTVORBIS`).
    NotVorbis,
    /// A Vorbis header that is damaged, or not the one expected
    /// (`OV_EBADHEADER`).
    BadHeader,
    /// An identification header of a Vorbis version other than 0
    /// (`OV_EVERSION`).
    Version,
    /// A packet that is not an audio packet (`OV_ENOTAUDIO`).
    NotAudio,
    /// An audio packet whose mode or window bits are damaged
    /// (`OV_EBADPACKET`).
    BadPacket,
}

impl Error {
    /// Tremor's number for the error (`OV_EBADHEADER` and so on).
    pub const fn code(self) -> i32 {
        match self {
            Self::BufferTooSmall => -131,
            Self::NotVorbis => -132,
            Self::BadHeader => -133,
            Self::Version => -134,
            Self::NotAudio => -135,
            Self::BadPacket => -136,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BufferTooSmall => "output buffer too small",
            Self::NotVorbis => "not a Vorbis header",
            Self::BadHeader => "damaged Vorbis header",
            Self::Version => "unsupported Vorbis version",
            Self::NotAudio => "not an audio packet",
            Self::BadPacket => "damaged audio packet",
        })
    }
}

impl std::error::Error for Error {}
