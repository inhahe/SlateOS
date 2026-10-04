//! A VP8 video decoder: libvpx's, ported to Rust.
//!
//! VP8 is the video format of the first WebM files and of WebRTC's early
//! years, and still of much of the web's older video. This crate turns its
//! compressed frames back into pictures exactly as libvpx -- Google's
//! reference decoder -- does: decoding libvpx's 62 test vectors, every
//! picture of every one hashes to the MD5 libvpx publishes for it
//! (`tests/vectors.rs`). All of VP8 is here: its four versions (the six-tap
//! or bilinear filter, the normal or simple loop filter, whole-pixel chroma),
//! segmentation, up to eight token partitions, golden and altref references
//! with their sign bias, probability updates kept or discarded per frame,
//! hidden frames, and key frames that change the picture's size. Why a port
//! and not libvpx itself is `design-decisions.md` §1339, written for VP9,
//! which this follows.
//!
//! ```no_run
//! # fn frames() -> Vec<Vec<u8>> { Vec::new() }
//! let mut decoder = vp8::Decoder::new();
//! for frame in frames() {
//!     // One frame from the container: a block of a WebM file, say.
//!     if let Some(picture) = decoder.decode(&frame)? {
//!         let y = picture.plane(0);
//!         # let _ = y;
//!     }
//! }
//! # Ok::<(), vp8::Error>(())
//! ```
//!
//! # How a frame is coded, and where each part lives
//!
//! - `header`: the uncompressed tag every frame starts with, and a key
//!   frame's start code and size.
//! - `boolread`: the arithmetic decoder everything after it is coded with.
//! - `decodeframe`: the frame header, the partitions, and each macroblock's
//!   reconstruction, row by row; and what lasts from frame to frame.
//! - `modes`: every macroblock's modes and motion vectors, which the first
//!   partition codes before any coefficient.
//! - `tokens`: a macroblock's coefficients.
//! - `idct`: the inverse transforms.
//! - `intra`: prediction from the pixels around a block.
//! - `inter`: motion-compensated prediction from reference frames.
//! - `loopfilter`: smoothing across block edges.
//! - `frame`: the planes frames are decoded into, borders and all.
//! - `decoder`: the frame loop -- the four buffers the frame being decoded
//!   and the three references share, and which frames show.
//!
//! The decoder is libvpx's single-threaded one. (libvpx can decode
//! macroblock rows on several threads when a stream has several token
//! partitions; its pictures are the same either way.)
//!
//! # What a hostile stream can do
//!
//! Decode to garbage, or fail with an [`Error`]; never panic, never read or
//! write outside its buffers. What libvpx does, frame by frame, this does:
//! a damaged frame is decoded as far as libvpx decodes it and marked
//! [`Picture::corrupted`], as libvpx marks it, and frames that predict from
//! it are marked too; the frames libvpx refuses are refused, and the
//! pictures it shows are the same pictures (`tests/damage.rs` holds the two
//! to it on 446 damaged streams). Where libvpx's behaviour depends on which
//! of its buffers holds what -- a reference copied from a buffer that does
//! not exist, and a version-3 macroblock whose chroma it leaves unpredicted
//! -- this keeps libvpx's buffers in the same order. Two differences, both in
//! what is reported rather than what is decoded: that copy from no buffer is
//! an [`Error::Corrupt`] here, where libvpx returns success and shows
//! nothing; and an empty frame does nothing here, where libvpx refuses one
//! that is not its null "flush" call. A frame larger than
//! [`DEFAULT_MAX_PIXELS`] is refused unless the decoder was made with
//! [`Decoder::with_max_pixels`].
//!
//! Portions of this crate are translated into Rust from libvpx v1.17.0
//! (copyright the WebM project authors) and changed for this project; used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`). Each translated file names its sources.

#![forbid(unsafe_code)]

mod boolread;
mod decodeframe;
mod decoder;
mod frame;
mod header;
mod idct;
mod inter;
mod intra;
mod loopfilter;
mod modes;
// Generated from libvpx by `tools/gen_tables.py`, which formats what it
// writes, so regenerating and diffing compares like with like.
mod tables;
mod tokens;

pub use decoder::{DEFAULT_MAX_PIXELS, Decoder, Picture, PlaneView};
pub use header::{StreamInfo, peek_stream_info};

/// Why a frame could not be decoded.
///
/// The two kinds libvpx tells apart: a stream that asks for something this
/// decoder does not do, and a stream that is damaged. The words say which
/// rule was broken, for a person reading a log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream uses something this decoder does not support, or is not
    /// VP8: libvpx's `VPX_CODEC_UNSUP_BITSTREAM`.
    Unsupported(&'static str),
    /// The stream is damaged or truncated: libvpx's `VPX_CODEC_CORRUPT_FRAME`.
    Corrupt(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(why) => write!(f, "unsupported VP8 stream: {why}"),
            Self::Corrupt(why) => write!(f, "corrupt VP8 stream: {why}"),
        }
    }
}

impl std::error::Error for Error {}
