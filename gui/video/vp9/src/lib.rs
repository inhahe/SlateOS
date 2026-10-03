//! A VP9 video decoder: libvpx's, ported to Rust.
//!
//! VP9 is the video format of most WebM files and of much of the web's video.
//! This crate turns its compressed frames back into pictures, exactly as
//! libvpx -- Google's reference decoder, the one every browser's VP9 was
//! checked against -- does: the conformance test decodes libvpx's own 314
//! test vectors and compares a fingerprint of every frame with libvpx's
//! (`tests/conformance.rs`). Why a port and not libvpx itself, and why the
//! decoder before the encoder, is `design-decisions.md` §1339.
//!
//! # How a frame is coded, and where each part lives
//!
//! - [`bits`]: the uncompressed header's plain bits.
//! - [`boolread`]: the arithmetic decoder everything after it is coded with.
//!
//! # What a hostile stream can do
//!
//! Decode to garbage, or fail with an [`Error`]; never panic, never read or
//! write outside its buffers. Every index taken from the stream is bounded or
//! checked, and arithmetic a crafted stream could overflow wraps, as libvpx's
//! C does.
//!
//! Portions of this crate are translated into Rust from libvpx v1.17.0
//! (copyright the WebM project authors) and changed for this project; used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`). Each translated file names its sources.

#![forbid(unsafe_code)]

pub mod bits;
pub mod boolread;
pub mod common;
pub mod probs;
// Generated from libvpx by `tools/gen_tables.py`; rustfmt would reflow it, and
// the file must stay byte for byte what the generator writes so that it can
// be regenerated and diffed.
#[rustfmt::skip]
pub mod tables;

/// Why a frame could not be decoded.
///
/// The two kinds libvpx tells apart: a stream that asks for something this
/// decoder does not do, and a stream that is damaged. The words say which
/// rule was broken, for a person reading a log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream uses something this decoder does not support: libvpx's
    /// `VPX_CODEC_UNSUP_BITSTREAM`.
    Unsupported(&'static str),
    /// The stream is damaged or truncated: libvpx's `VPX_CODEC_CORRUPT_FRAME`.
    Corrupt(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(why) => write!(f, "unsupported VP9 stream: {why}"),
            Self::Corrupt(why) => write!(f, "corrupt VP9 stream: {why}"),
        }
    }
}

impl std::error::Error for Error {}
