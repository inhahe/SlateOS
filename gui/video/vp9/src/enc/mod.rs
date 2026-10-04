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
//! - `cpi`: the compressor's state, libvpx's `VP9_COMP`.
//! - `ratectrl`: one-pass constant-bitrate rate control.
//! - `aq_cyclicrefresh`: cyclic refresh, the realtime adaptive quantisation.
//! - `rd`: rate-distortion constants and mode costs.
//! - `partition`: variance-based partitioning.
//! - `pickmode`: the realtime mode search.
//! - `nonrd`: libvpx's realtime decisions, as a `Decide`.

pub(crate) mod aq_cyclicrefresh;
pub(crate) mod bitstream;
pub(crate) mod cost;
pub(crate) mod cpi;
pub(crate) mod encodeframe;
pub(crate) mod encoder;
pub(crate) mod fdct;
pub(crate) mod nonrd;
pub(crate) mod partition;
pub(crate) mod pickmode;
pub(crate) mod quantize;
pub(crate) mod ratectrl;
pub(crate) mod rd;
pub(crate) mod subexp;
pub(crate) mod tokenize;
pub(crate) mod writer;

pub use encoder::{Encoder, EncoderConfig};
