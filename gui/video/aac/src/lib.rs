//! AAC sound, decoded as FFmpeg's fixed-point decoder decodes it: a
//! translation of FFmpeg 8.1.3's `aac_fixed` (`libavcodec/aac/`), module by
//! module, held bit for bit to that decoder's output (design-decisions
//! §1377).
//!
//! Fixed point, so every sample has exactly one right value, the one
//! FFmpeg's C computes; its handling of damaged streams comes with it. The
//! object types it decodes are FFmpeg's fixed-point decoder's: AAC Main,
//! LC and LTP, their error-resilient forms with LD and ELD, and HE-AAC's
//! SBR and PS over them. USAC (xHE-AAC), which that decoder refuses, is
//! refused here too.
//!
//! What is translated so far, and from where:
//!
//! - [`mpeg4audio`]: the AudioSpecificConfig's common part
//!   (`mpeg4audio.c`).
//! - [`layout`]: which channel each element is, and the order FFmpeg's
//!   output gives them in (`aacdec.c`, `aacdec_tab.c`).
//! - [`context`]: the decoder's state and its channel elements, and how a
//!   configuration sets them up (`aacdec.h`, `aacdec.c`).
//! - [`softfloat`]: libavutil's integer floating point, which prediction,
//!   SBR and PS compute in (`softfloat.h`).
//! - [`tables`]: FFmpeg's constant tables, generated from its source by
//!   `tools/gen_tables.py`.
//!
//! The oracle the crate is held to is `tools/ffaac/build.sh`'s FFmpeg and
//! `tools/pcm_expect.py`'s record of what it decodes.

#![forbid(unsafe_code)]
#![allow(
    dead_code,
    reason = "translated a module at a time: what nothing calls yet is called once the \
              decoder above it is in, and this allow goes then"
)]

pub(crate) mod context;
pub(crate) mod layout;
pub(crate) mod mpeg4audio;
pub(crate) mod softfloat;
// Generated from FFmpeg by `tools/gen_tables.py`, which formats what it
// writes, so regenerating and diffing compares like with like.
pub(crate) mod tables;

/// Why a stream, or a packet of one, does not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The data is damaged, or not what its configuration says it is
    /// (FFmpeg's `AVERROR_INVALIDDATA`, and its other refusals of data).
    InvalidData,
    /// The stream uses something the standard has that FFmpeg's decoder
    /// does not decode (its `AVERROR_PATCHWELCOME` and `ENOSYS`).
    Unsupported,
    /// The decoder broke one of its own rules: a bug, not the stream's fault
    /// (`AVERROR_BUG`).
    Bug,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid AAC data",
            Self::Unsupported => "an AAC feature the decoder does not support",
            Self::Bug => "an AAC decoder bug",
        })
    }
}

impl std::error::Error for Error {}
