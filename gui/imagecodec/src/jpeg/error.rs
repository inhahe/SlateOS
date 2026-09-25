//! libjpeg's fatal errors (`ERREXIT`), as values.
//!
//! libjpeg reports a fatal error by calling the error manager's `error_exit`,
//! which every caller that matters here -- libtiff, Chrome, Pillow -- turns
//! into "this datastream cannot be decoded". Which error it was changes no
//! caller's behaviour, only its message, so the variants carry libjpeg's name
//! for the condition (`JERR_...`) as the message and are grouped by what kind
//! of refusal they are.

use crate::ImageError;

/// Why a datastream cannot be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    /// The datastream contradicts the format: a length that does not add up,
    /// a table that is not a table, a marker where none may be.
    Malformed(&'static str),
    /// A legal datastream using something libjpeg itself does not implement,
    /// or that the caller's output does not take (12-bit samples through the
    /// 8-bit interface, hierarchical coding).
    Unsupported(&'static str),
    /// Decoding it would take more than the caller allowed.
    TooLarge {
        /// What it would take: pixels, or bytes.
        amount: u64,
        /// What was allowed.
        limit: u64,
    },
}

impl From<Error> for ImageError {
    fn from(error: Error) -> Self {
        match error {
            Error::Malformed(what) => Self::Malformed(what),
            Error::Unsupported(what) => Self::Unsupported(what),
            Error::TooLarge { amount, limit } => Self::TooLarge {
                pixels: amount,
                limit,
            },
        }
    }
}

/// A shorthand for the variants, one per `JERR_` code this port raises.
pub(super) mod jerr {
    use super::Error;

    pub(in crate::jpeg) const NO_SOI: Error = Error::Malformed("JPEG: not a JPEG file (no SOI)");
    pub(in crate::jpeg) const SOI_DUPLICATE: Error = Error::Malformed("JPEG: two SOI markers");
    pub(in crate::jpeg) const SOF_DUPLICATE: Error = Error::Malformed("JPEG: two SOF markers");
    pub(in crate::jpeg) const SOF_UNSUPPORTED: Error =
        Error::Unsupported("JPEG: hierarchical or differential coding (SOF5-7, 13-15)");
    pub(in crate::jpeg) const SOF_NO_SOS: Error =
        Error::Malformed("JPEG: a frame with no scan (SOF before EOI)");
    pub(in crate::jpeg) const SOS_NO_SOF: Error = Error::Malformed("JPEG: a scan before its frame");
    pub(in crate::jpeg) const EMPTY_IMAGE: Error =
        Error::Malformed("JPEG: a frame with a zero dimension or no components");
    pub(in crate::jpeg) const BAD_LENGTH: Error = Error::Malformed("JPEG: a bogus marker length");
    pub(in crate::jpeg) const BAD_COMPONENT_ID: Error =
        Error::Malformed("JPEG: a scan naming a component the frame lacks");
    pub(in crate::jpeg) const BAD_HUFF_TABLE: Error = Error::Malformed("JPEG: a bogus Huffman table");
    pub(in crate::jpeg) const DHT_INDEX: Error =
        Error::Malformed("JPEG: a Huffman table numbered past 3");
    pub(in crate::jpeg) const DQT_INDEX: Error =
        Error::Malformed("JPEG: a quantisation table numbered past 3");
    pub(in crate::jpeg) const DAC_INDEX: Error =
        Error::Malformed("JPEG: an arithmetic table numbered past 15");
    pub(in crate::jpeg) const DAC_VALUE: Error =
        Error::Malformed("JPEG: arithmetic DC conditioning with L above U");
    pub(in crate::jpeg) const UNKNOWN_MARKER: Error = Error::Malformed("JPEG: an unknown marker");
    pub(in crate::jpeg) const NO_IMAGE: Error =
        Error::Malformed("JPEG: tables only, where an image was expected");
    pub(in crate::jpeg) const EOI_EXPECTED: Error =
        Error::Malformed("JPEG: a second scan in a single-scan image");
    pub(in crate::jpeg) const IMAGE_TOO_BIG: Error =
        Error::Malformed("JPEG: wider or taller than 65500");
    pub(in crate::jpeg) const BAD_PRECISION: Error =
        Error::Unsupported("JPEG: sample precision other than 8 bits");
    pub(in crate::jpeg) const COMPONENT_COUNT: Error =
        Error::Malformed("JPEG: more components than a frame or scan may have");
    pub(in crate::jpeg) const BAD_SAMPLING: Error =
        Error::Malformed("JPEG: a sampling factor outside 1-4");
    pub(in crate::jpeg) const BAD_MCU_SIZE: Error =
        Error::Malformed("JPEG: more than ten blocks in an MCU");
    pub(in crate::jpeg) const NO_QUANT_TABLE: Error =
        Error::Malformed("JPEG: a component naming a quantisation table never defined");
    pub(in crate::jpeg) const NO_HUFF_TABLE: Error =
        Error::Malformed("JPEG: a scan naming a Huffman table never defined");
    pub(in crate::jpeg) const NO_ARITH_TABLE: Error =
        Error::Malformed("JPEG: a scan naming an arithmetic table past 15");
    pub(in crate::jpeg) const BAD_PROGRESSION: Error =
        Error::Malformed("JPEG: impossible progressive scan parameters");
    pub(in crate::jpeg) const BAD_DCT_COEF: Error =
        Error::Malformed("JPEG: a DC coefficient out of range");
    pub(in crate::jpeg) const BAD_J_COLORSPACE: Error =
        Error::Malformed("JPEG: a component count its colour space cannot have");
    pub(in crate::jpeg) const CONVERSION_NOTIMPL: Error =
        Error::Unsupported("JPEG: a colour conversion libjpeg does not do");
    pub(in crate::jpeg) const FRACT_SAMPLE_NOTIMPL: Error =
        Error::Unsupported("JPEG: sampling factors that are not whole multiples");
    pub(in crate::jpeg) const TOO_MANY_SCANS: Error =
        Error::Malformed("JPEG: more scans than a decoder should take");
}
