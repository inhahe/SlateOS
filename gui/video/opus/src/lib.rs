//! An Opus decoder (RFC 6716, with RFC 8251's corrections): libopus 1.5.2's
//! fixed-point decoder, translated into safe Rust.
//!
//! Opus is the sound of most WebM files and of most voice calls on the web.
//! A packet is one of three codecs, or two at once: SILK (speech, from
//! Skype: linear prediction, 8 to 16 kHz), CELT (music: a transform codec
//! up to 48 kHz) or hybrid (SILK below 8 kHz, CELT above), in frames of 2.5
//! to 60 ms, mono or stereo; a stream can switch between them packet by
//! packet. This decodes all of it, with libopus's concealment of lost
//! packets, its in-band FEC (a packet's low-rate copy of the one before) and
//! DTX; and streams of more than two channels, several Opus streams in one
//! packet (RFC 7845's channel mapping families: surround, ambisonics with or
//! without a demixing matrix, and discrete channels).
//!
//! **Bit-exact with libopus.** libopus's fixed-point build computes in
//! integers, so it has one right answer on every machine, and this port
//! gives the same samples to the bit. `tests/streams.rs` holds it to that
//! over streams libopus's encoder made of signals `tools/make_streams.c`
//! synthesises (every mode, bandwidth and frame size, the switches between
//! them, DTX, FEC, multistream), each decoded at every output rate and
//! channel count -- intact, with packets lost, damaged, and with each
//! decoder setting -- digest for digest and error for error with libopus's
//! own decoders (`tools/references.py`); and the RFC 8251 test vectors the
//! same way when `OPUS_VECTORS` names them. Where libopus's C arithmetic
//! wraps, this wraps on purpose, so a hostile packet decodes to the noise
//! libopus makes of it, or an error, never a panic (`tests/damage.rs`).
//!
//! **Using it.** A [`Decoder`] decodes one stream's packets (`opus_decode`):
//!
//! ```
//! # fn main() -> Result<(), opus::Error> {
//! let mut decoder = opus::Decoder::new(48000, 2)?;
//! // Room for 120 ms, the longest a packet can hold.
//! let mut pcm = vec![0i16; 5760 * 2];
//! // A CELT fullband stereo packet: a TOC byte, then the frame (any bytes
//! // decode to something).
//! let packet = [0xFC, 0x12, 0x34, 0x56];
//! let samples = decoder.decode(Some(&packet), &mut pcm, false)?;
//! assert_eq!(samples, 960); // 20 ms a channel
//! // A lost packet, concealed: as long as the last.
//! let n = decoder.last_packet_duration();
//! decoder.decode(None, &mut pcm[..n * 2], false)?;
//! # Ok(())
//! # }
//! ```
//!
//! A file's stream starts with an `OpusHead` (in Ogg, the first packet; in
//! Matroska, the track's `CodecPrivate`): [`Head::parse`] reads it and
//! [`Head::decoder`] makes the decoder it needs ([`AnyDecoder`]: a
//! [`Decoder`], a [`MultistreamDecoder`] or a [`ProjectionDecoder`]), at its
//! output gain. The head's `pre_skip` samples at the start are the
//! encoder's delay, not sound, and are the caller's to drop; so is the
//! pre-roll a seek needs -- decode from 80 ms before the time sought and
//! drop what comes before it (RFC 7845 §4.6), since a packet decoded cold
//! is not yet the sound it will be.
//!
//! **What is not here.** An encoder; and libopus's optional neural
//! extensions (DRED, the deep PLC, OSCE), off in libopus's default build,
//! which need megabytes of model weights. Decoding with them off is
//! libopus's default behaviour, and what this matches.
//!
//! **The parts.** `celt` (the transform codec: band energies, PVQ shapes,
//! the inverse MDCT, the pitch post-filter, concealment); `silk` (the
//! speech codec: LPC synthesis with long-term prediction, its concealment
//! and comfort noise, the stereo unmixing, the resampler); `decoder` (the
//! Opus layer: packets, mode transitions, redundancy frames, gain);
//! `multistream`; `packet` (the TOC and framing); `entdec` (the range
//! decoder); `fixed` (libopus's fixed-point macros). Each file names the
//! libopus files it was translated from.

#![forbid(unsafe_code)]

mod celt;
mod decoder;
mod entdec;
mod error;
mod fixed;
mod multistream;
mod packet;
mod silk;
#[cfg(test)]
mod testutil;

pub use decoder::Decoder;
pub use error::Error;
pub use multistream::{AnyDecoder, Head, MultistreamDecoder, ProjectionDecoder};
pub use packet::{Bandwidth, packet_has_lbrr, packet_samples};
