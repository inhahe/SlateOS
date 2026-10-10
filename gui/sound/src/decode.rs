//! A sound file decoded into [`Audio`]: WAV ([`crate::wav`]), or Ogg -- its
//! first Vorbis or Opus stream -- decoded by lane F's ports of Xiph's own
//! decoders (`gui/video/{ogg,vorbis,opus}`).
//!
//! The freedesktop sound theme specification has a theme's sounds as Ogg
//! Vorbis (`.oga`, `.ogg`) or WAV, and Opus is how an Ogg file is usually
//! made now. Other codecs in an Ogg file -- FLAC, Speex -- are refused
//! rather than half-supported.
//!
//! A sound is one link: a chained Ogg file stops at its first link's end.
//! A damaged packet is passed over, as players pass over it, rather than
//! costing the rest of the sound. And a sound longer than
//! [`MAX_SECONDS`] is cut there, before the rest is decoded.

use crate::convert::ChannelOrder;
use crate::{Audio, MAX_SECONDS};
use ogg::{Codec, Demuxer, Stream};
use std::fmt;
use std::io::Cursor;

/// The most samples a channel one Opus packet decodes to: 120 ms at 48 kHz.
const OPUS_MAX_FRAME: usize = 5760;

/// Opus's own rate, at which it is decoded: the mixer's too.
const OPUS_RATE: u32 = 48_000;

/// Why a file could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Neither WAV nor Ogg.
    Unrecognised,
    /// A WAV file this cannot read; the words say why.
    Wav(&'static str),
    /// An Ogg file that cannot be read.
    Ogg(ogg::Error),
    /// An Ogg file with no Vorbis or Opus stream.
    NoAudioStream,
    /// A Vorbis stream its decoder refuses.
    Vorbis(vorbis::Error),
    /// An Opus stream whose head cannot be read.
    OpusHead,
    /// An Opus stream its decoder refuses.
    Opus(opus::Error),
    /// A stream with a channel count or rate this does not play.
    Shape,
    /// A stream that decodes to no sound at all.
    Empty,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrecognised => write!(f, "not a sound file this plays (WAV or Ogg)"),
            Self::Wav(why) => write!(f, "a WAV file this cannot read: {why}"),
            Self::Ogg(e) => write!(f, "{e}"),
            Self::NoAudioStream => write!(f, "an Ogg file with no Vorbis or Opus sound in it"),
            Self::Vorbis(e) => write!(f, "a Vorbis stream that cannot be decoded: {e}"),
            Self::OpusHead => write!(f, "an Opus stream whose head cannot be read"),
            Self::Opus(e) => write!(f, "an Opus stream that cannot be decoded: {e}"),
            Self::Shape => write!(f, "a sound with no channels, more than eight, or no rate"),
            Self::Empty => write!(f, "a sound file with no sound in it"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The sound file in `bytes`.
///
/// # Errors
///
/// [`DecodeError::Unrecognised`] for neither WAV nor Ogg, and whatever
/// reading the one it is met.
pub fn decode(bytes: &[u8]) -> Result<Audio, DecodeError> {
    if crate::wav::is_wav(bytes) {
        crate::wav::read(bytes)
    } else if ogg::probe(bytes) {
        decode_ogg(bytes)
    } else {
        Err(DecodeError::Unrecognised)
    }
}

/// The Ogg file in `bytes`: its first Vorbis or Opus stream.
fn decode_ogg(bytes: &[u8]) -> Result<Audio, DecodeError> {
    let mut demuxer = Demuxer::open(Cursor::new(bytes)).map_err(DecodeError::Ogg)?;
    let (index, stream) = demuxer
        .streams()
        .iter()
        .enumerate()
        .find(|(_, s)| matches!(s.codec, Codec::Vorbis | Codec::Opus))
        .map(|(i, s)| (i, s.clone()))
        .ok_or(DecodeError::NoAudioStream)?;
    let mut decoder = Decoder::for_stream(&stream)?;
    let channels = decoder.channels();
    let rate = decoder.rate();
    let wide = u16::try_from(channels).map_err(|_| DecodeError::Shape)?;
    if wide == 0 || wide > crate::wav::MAX_CHANNELS || rate == 0 {
        return Err(DecodeError::Shape);
    }
    let most = usize::try_from(rate)
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(MAX_SECONDS).unwrap_or(usize::MAX))
        .saturating_mul(channels);
    let mut out = vec![0.0f32; decoder.max_samples().saturating_mul(channels)];
    let mut samples = Vec::new();
    while let Some(packet) = demuxer.next_packet().map_err(DecodeError::Ogg)? {
        if packet.stream != index {
            continue;
        }
        if packet.new_headers.is_some() {
            // The next link of a chained file: a sound is one.
            break;
        }
        let Some(n) = decoder.decode(&packet.data, &mut out)? else {
            // Damaged: passed over.
            continue;
        };
        let skip = usize::try_from(packet.skip_samples)
            .unwrap_or(usize::MAX)
            .min(n);
        let end = n
            .saturating_sub(usize::try_from(packet.discard_padding).unwrap_or(usize::MAX))
            .max(skip);
        let decoded = out
            .get(skip.saturating_mul(channels)..end.saturating_mul(channels))
            .unwrap_or(&[]);
        samples.extend_from_slice(decoded);
        if samples.len() >= most {
            samples.truncate(most);
            break;
        }
    }
    if samples.is_empty() {
        return Err(DecodeError::Empty);
    }
    Ok(Audio {
        rate,
        channels: wide,
        samples,
        order: decoder.order(),
    })
}

/// A stream's decoder, Vorbis or Opus.
enum Decoder {
    Vorbis(Box<vorbis::Decoder>),
    Opus {
        decoder: Box<opus::AnyDecoder>,
        family: u8,
    },
}

impl Decoder {
    /// The decoder for `stream`, from its headers.
    fn for_stream(stream: &Stream) -> Result<Self, DecodeError> {
        match stream.codec {
            Codec::Vorbis => {
                // Identification, comment, setup.
                let (Some(id), Some(setup)) = (stream.headers.first(), stream.headers.get(2))
                else {
                    return Err(DecodeError::Vorbis(vorbis::Error::BadHeader));
                };
                vorbis::Decoder::new(id, setup)
                    .map(|d| Self::Vorbis(Box::new(d)))
                    .map_err(DecodeError::Vorbis)
            }
            Codec::Opus => {
                let head = stream
                    .headers
                    .first()
                    .and_then(|h| opus::Head::parse(h))
                    .ok_or(DecodeError::OpusHead)?;
                let decoder = head.decoder(OPUS_RATE).map_err(DecodeError::Opus)?;
                Ok(Self::Opus {
                    decoder: Box::new(decoder),
                    family: head.mapping_family,
                })
            }
            _ => Err(DecodeError::NoAudioStream),
        }
    }

    fn channels(&self) -> usize {
        match self {
            Self::Vorbis(d) => d.info().channels,
            Self::Opus { decoder, .. } => decoder.channels(),
        }
    }

    fn rate(&self) -> u32 {
        match self {
            Self::Vorbis(d) => d.info().rate,
            Self::Opus { .. } => OPUS_RATE,
        }
    }

    /// The most samples a channel one packet decodes to.
    fn max_samples(&self) -> usize {
        match self {
            Self::Vorbis(d) => d.max_samples(),
            Self::Opus { .. } => OPUS_MAX_FRAME,
        }
    }

    /// Which channel is which past two.
    fn order(&self) -> ChannelOrder {
        match self {
            Self::Vorbis(_) | Self::Opus { family: 0 | 1, .. } => ChannelOrder::Vorbis,
            Self::Opus { .. } => ChannelOrder::Unknown,
        }
    }

    /// `packet` decoded into `out`, interleaved: the samples a channel, or
    /// `None` for a damaged packet, which is passed over.
    ///
    /// # Errors
    ///
    /// A refusal that is not about one packet -- the decoder's own failure.
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<Option<usize>, DecodeError> {
        match self {
            Self::Vorbis(d) => match d.decode_float(packet, out) {
                Ok(n) => Ok(Some(n)),
                Err(vorbis::Error::NotAudio | vorbis::Error::BadPacket) => Ok(None),
                Err(e) => Err(DecodeError::Vorbis(e)),
            },
            Self::Opus { decoder, .. } => match decoder.decode_float(Some(packet), out, false) {
                Ok(n) => Ok(Some(n)),
                Err(opus::Error::InvalidPacket) => Ok(None),
                Err(e) => Err(DecodeError::Opus(e)),
            },
        }
    }
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
