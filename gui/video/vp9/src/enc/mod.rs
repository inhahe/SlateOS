//! The VP9 encoder: libvpx's, ported as the decoder was, its realtime path
//! first -- what SlateOS's remote desktop needs to film the screen.
//!
//! - `writer`: the bool encoder and the uncompressed header's bit writer.
//! - `bitstream`: the frame as bits -- headers, partitions, modes, tokens.
//! - `cost`: what coding a decision costs, in 1/512 bits.
//! - `subexp`: whether to update a probability, and how the update is coded.
//! - `fdct`: the forward transforms.
//! - `quantize`: the quantisers and their tables.
//! - `tokenize`: quantised coefficients as the tokens that code them.
//! - `encodeframe`: a frame's blocks predicted, coded and reconstructed.
//! - `encoder`: the frame loop and the public [`Encoder`].

pub(crate) mod bitstream;
pub(crate) mod cost;
pub(crate) mod encodeframe;
pub(crate) mod encoder;
pub(crate) mod fdct;
pub(crate) mod quantize;
pub(crate) mod subexp;
pub(crate) mod tokenize;
pub(crate) mod writer;

pub use encoder::{Encoder, EncoderConfig};
