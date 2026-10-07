//! FLAC: libFLAC 1.5.0's decoder, ported to Rust and held to libFLAC sample
//! for sample.
//!
//! FLAC is lossless: a stream decodes to exactly the samples its encoder was
//! given, so a decoder is right or wrong, and this one is held to libFLAC on
//! every stream its tests have -- every bit depth from 4 to 32, one to eight
//! channels, every predictor, every block size -- and on damaged ones too,
//! where what libFLAC does with the damage (which frames it gives up, where
//! it finds the next, the silence it puts in for frames lost between two it
//! decoded) is what this does.
//!
//! - [`Reader`] reads a native FLAC file (`.flac`): its metadata
//!   ([`Metadata`]: the stream's facts, its tags, its cover art) and then its
//!   frames in order, found by their sync codes, with a seek to any sample.
//! - [`Decoder`] decodes one frame at a time, for a container that carries
//!   them as packets: Ogg FLAC, Matroska's `A_FLAC`, MP4's `fLaC`.
//!
//! Samples come as `i32`s at the stream's own bit depth, a slice a channel.
//!
//! Translated from libFLAC (Xiph's reference library), copyright Josh
//! Coalson and Xiph.Org, used under its BSD licence (`licenses/flac-COPYING`);
//! each module names the files it came from.

#![forbid(unsafe_code)]

mod bits;
mod crc;
mod frame;
mod metadata;
mod reader;

use std::fmt;

pub use frame::{ChannelAssignment, Header, Status};
pub use metadata::{Block, Comments, Metadata, Picture, SeekPoint, StreamInfo};
pub use reader::{Event, Frame, Reader};

use frame::{Decoded, FrameDecoder};

/// Why a file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed.
    Io(std::io::ErrorKind),
    /// The file has no `fLaC` marker and no frame.
    NotFlac,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.kind())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "the FLAC file cannot be read: {kind}"),
            Self::NotFlac => f.write_str("not a FLAC file"),
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LostSync => "lost sync",
            Self::BadHeader => "damaged frame header",
            Self::CrcMismatch => "frame CRC mismatch",
            Self::Unparseable => "unparseable frame",
            Self::BadMetadata => "damaged metadata block",
            Self::OutOfBounds => "samples out of bounds",
            Self::MissingFrame => "missing frame",
        })
    }
}

impl std::error::Error for Status {}

/// Decodes frames a container gives one a packet.
#[derive(Clone, Debug, Default)]
pub struct Decoder {
    frame: FrameDecoder,
}

impl Decoder {
    /// A decoder for the stream `info` describes (a frame may leave its
    /// sample rate and bit depth to it), or for one with none.
    pub fn new(info: Option<&StreamInfo>) -> Self {
        let mut frame = FrameDecoder::default();
        frame.defaults = info.map(StreamInfo::defaults);
        Self { frame }
    }

    /// Decodes the frame `packet` holds: its header and each channel's
    /// samples.
    ///
    /// # Errors
    ///
    /// The [`Status`] libFLAC would report for it: a damaged header, a
    /// broken rule, a CRC that does not match, samples out of bounds, or
    /// [`Status::LostSync`] for a packet that is not a whole frame.
    pub fn decode(&mut self, packet: &[u8]) -> Result<(Header, Vec<&[i32]>), Status> {
        let starts_with_sync =
            packet.first() == Some(&0xff) && packet.get(1).is_some_and(|b| b >> 1 == 0x7c);
        if !starts_with_sync {
            return Err(Status::LostSync);
        }
        match self.frame.decode(packet) {
            Decoded::Frame { header, .. } => {
                let block = header.block_size as usize;
                let channels = self
                    .frame
                    .output
                    .iter()
                    .take(header.channels as usize)
                    .map(|c| c.get(..block).unwrap_or_default())
                    .collect();
                Ok((header, channels))
            }
            Decoded::Error { status, .. } => Err(status),
            Decoded::NeedMore { .. } => Err(Status::LostSync),
        }
    }
}
