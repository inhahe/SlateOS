//! H.264 video -- AVC, MPEG-4 Part 10, the video of most MP4 files, of
//! phones, cameras and much of the web -- decoded as FFmpeg's decoder
//! decodes it: FFmpeg 8.1.3's `libavcodec` H.264 decoder translated into
//! Rust, module by module (the operator's choice, design-decisions/1376,
//! F-Q8), and held to FFmpeg picture for picture -- the conformance
//! streams' frame checksums FFmpeg's own tests keep, and FFmpeg's output on
//! files of every kind.
//!
//! # Where each part of FFmpeg lives
//!
//! - `bits`: `get_bits.h` and `golomb.h` -- a NAL unit's bits and the
//!   Exp-Golomb codes most syntax is written in.
//! - `nal`: `h2645_parse.c` -- a packet split into NAL units, after start
//!   codes or after lengths, and unescaped.
//! - `ps`: `h264_ps.c` and `h2645_vui.c` -- sequence and picture parameter
//!   sets; `tables` the data tables they and the rest read.
//! - `sei`: `h264_sei.c` and `h2645_sei.c` -- supplemental enhancement
//!   information: recovery points, the x264 version, picture timing,
//!   captions, film grain, and what is about the picture rather than in it.
//! - `slice`: `h264_slice_header_parse` in `h264_slice.c`, with the
//!   reference counts and weights of `h264_parse.c` and the list
//!   modifications and marking of `h264_refs.c` -- a slice's header.
//!
//! The rest is being translated in this order, each part against FFmpeg's
//! output before the next: pictures (`h264_slice.c`, `h264_picture.c`),
//! references (`h264_refs.c`), CAVLC and CABAC (`h264_cavlc.c`, `cabac.c`,
//! `h264_cabac.c`), motion vector prediction (`h264_mvpred.h`,
//! `h264_direct.c`), reconstruction (`h264_mb.c` and its templates, with
//! `h264pred`, `h264qpel`, `h264chroma`, `h264idct` and `h264dsp`), the
//! loop filter (`h264_loopfilter.c`), error concealment
//! (`error_resilience.c`), and the decoder that drives them (`h264dec.c`).

#![forbid(unsafe_code)]
// The decoder is translated a module at a time, and each module's parts are
// read only by the next ones and by its tests until the decoder that drives
// them is in (`todo.txt`: "h264: the crate-level dead_code allowance").
#![allow(
    dead_code,
    reason = "translated module by module; nothing drives the parts until the decoder is in"
)]

mod bits;
mod nal;
mod ps;
mod sei;
mod slice;
mod tables;
#[cfg(test)]
mod writer;

/// Why a packet or a stream could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The data is not H.264 where FFmpeg needs it to be
    /// (`AVERROR_INVALIDDATA`).
    InvalidData,
    /// A size past what FFmpeg's arithmetic can hold (`AVERROR(ERANGE)`,
    /// `AVERROR(ENOMEM)` for an allocation that large).
    TooLarge,
    /// Valid H.264 that FFmpeg does not decode either -- slice groups, an
    /// 11- or 13-bit depth (`AVERROR_PATCHWELCOME`).
    Unsupported,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid H.264 data",
            Self::TooLarge => "an H.264 size too large to decode",
            Self::Unsupported => "H.264 that FFmpeg does not decode either",
        })
    }
}

impl std::error::Error for Error {}
