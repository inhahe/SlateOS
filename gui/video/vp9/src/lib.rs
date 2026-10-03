//! A VP9 video decoder: libvpx's, ported to Rust.
//!
//! VP9 is the video format of most WebM files and of much of the web's video.
//! This crate turns its compressed frames back into pictures exactly as
//! libvpx -- Google's reference decoder, the one every browser's VP9 was
//! checked against -- does: decoding libvpx's 314 conformance vectors, every
//! picture of every one hashes to the MD5 libvpx publishes for it
//! (`tests/conformance.rs`). All of VP9 is here: profiles 0 to 3 (8, 10 and
//! 12 bits; 4:2:0, 4:2:2, 4:4:0 and 4:4:4), tiles, segmentation, lossless
//! frames, compound prediction, references of other sizes, intra-only frames
//! and frames shown from a reference slot. Why a port and not libvpx itself,
//! and why the decoder before the encoder, is `design-decisions.md` §1339.
//!
//! ```no_run
//! # fn packets() -> Vec<Vec<u8>> { Vec::new() }
//! let mut decoder = vp9::Decoder::new();
//! for packet in packets() {
//!     // One packet from the container: a frame, or a superframe of several.
//!     if let Some(picture) = decoder.decode(&packet)? {
//!         let y = picture.plane8(0); // or plane16 for 10- and 12-bit streams
//!         # let _ = y;
//!     }
//! }
//! # Ok::<(), vp9::Error>(())
//! ```
//!
//! # How a frame is coded, and where each part lives
//!
//! - `bits`: the uncompressed header's plain bits.
//! - `boolread`: the arithmetic decoder everything after it is coded with.
//! - `header`: the frame header's fields, and the superframe index that packs
//!   several frames into one packet.
//! - `probs`: the probabilities everything is decoded with, and how each frame
//!   adapts them.
//! - `decoder`: the frame loop -- reference slots, saved probability
//!   contexts, what each frame leaves for the next, and which frames show.
//! - `block`: tiles, partitions, each block's mode information and motion
//!   vectors, and its reconstruction.
//! - `detokenize`: a transform block's coefficients.
//! - `idct`: the inverse transforms.
//! - `intra`: prediction from the pixels around a block.
//! - `inter`: motion-compensated prediction from reference frames.
//! - `loopfilter`: smoothing across block edges.
//! - `frame`: the planes frames are decoded into.
//!
//! Not yet here: threading (libvpx decodes tile columns, and loop-filters
//! rows, in parallel), SIMD (every filter and transform is libvpx's C, in
//! Rust), and reuse of frame buffers between frames.
//!
//! # What a hostile stream can do
//!
//! Decode to garbage, or fail with an [`Error`]; never panic, never read or
//! write outside its buffers. Every index taken from the stream is bounded or
//! checked, and arithmetic a crafted stream could overflow wraps. A frame
//! larger than [`DEFAULT_MAX_PIXELS`] (VP9's level 6.2) is refused unless the
//! decoder was made with [`Decoder::with_max_pixels`].
//!
//! Portions of this crate are translated into Rust from libvpx v1.17.0
//! (copyright the WebM project authors) and changed for this project; used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`). Each translated file names its sources.

#![forbid(unsafe_code)]

mod bits;
mod block;
mod boolread;
mod common;
mod decoder;
mod detokenize;
mod frame;
mod header;
mod idct;
mod inter;
mod intra;
mod loopfilter;
mod probs;
// Generated from libvpx by `tools/gen_tables.py`, which formats what it
// writes, so regenerating and diffing compares like with like.
mod tables;

pub use decoder::{DEFAULT_MAX_PIXELS, Decoder, Picture, PlaneView};
pub use header::{StreamInfo, parse_superframe_index, peek_stream_info};

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
