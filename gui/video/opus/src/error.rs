//! What can go wrong decoding Opus: libopus's error codes, as a type.

use std::fmt;

/// Why a decoder could not be made, or a packet decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// An argument out of range: a sampling rate or channel count Opus has
    /// none of, or a frame size that is not one (`OPUS_BAD_ARG`).
    BadArgument,
    /// The output buffer is too small for the packet's samples
    /// (`OPUS_BUFFER_TOO_SMALL`).
    BufferTooSmall,
    /// A frame read past its own end: the packet is damaged in a way the
    /// decoder only notices after decoding it (`OPUS_INTERNAL_ERROR`).
    Internal,
    /// The packet is not a valid Opus packet (`OPUS_INVALID_PACKET`).
    InvalidPacket,
}

impl Error {
    /// libopus's number for the error (`OPUS_BAD_ARG` and so on).
    pub const fn code(self) -> i32 {
        match self {
            Self::BadArgument => -1,
            Self::BufferTooSmall => -2,
            Self::Internal => -3,
            Self::InvalidPacket => -4,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadArgument => "invalid argument",
            Self::BufferTooSmall => "buffer too small",
            Self::Internal => "internal error",
            Self::InvalidPacket => "corrupted stream",
        })
    }
}

impl std::error::Error for Error {}
