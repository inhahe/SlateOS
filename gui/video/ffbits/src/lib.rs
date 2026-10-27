//! Bits and variable-length codes read as FFmpeg's libavcodec reads them,
//! for the decoders translated from it (`gui/video/h264`, `gui/video/aac`).
//!
//! A translated decoder is held to FFmpeg's output on damaged streams as on
//! sound ones, and what a damaged stream decodes to depends on how the bits
//! are read where they run out and what a pattern that is no code reads as:
//! [`Bits`] reads past its end as FFmpeg's padded buffers do, and [`Vlc`]
//! builds its lookup tables as `vlc.c` does, so an invalid code takes the
//! bits and answers the -1 FFmpeg's does.
//!
//! - [`bits`]: `get_bits.h`'s `GetBitContext`, and `golomb.h`'s Exp-Golomb
//!   codes.
//! - [`vlc`]: `vlc.c`'s tables and `get_vlc2`.

pub mod bits;
pub mod vlc;

pub use bits::Bits;
pub use vlc::{Vlc, VlcError};
