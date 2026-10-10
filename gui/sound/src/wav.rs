//! WAV files read: RIFF's `fmt ` and `data` chunks -- integer samples of 8,
//! 16, 24 or 32 bits and floating point of 32 or 64, plain or
//! `WAVE_FORMAT_EXTENSIBLE` -- into [`Audio`].
//!
//! Nothing in a file is trusted. Every size is checked against the bytes
//! actually there, and a chunk that claims more than remains is refused --
//! but for `data`, whose size a streaming writer often leaves at nought or
//! at `0xFFFF_FFFF`: then the rest of the file is taken, whole frames of it.
//! A sound longer than [`MAX_SECONDS`] is cut there.

use crate::convert::ChannelOrder;
use crate::decode::DecodeError;
use crate::{Audio, MAX_SECONDS};

/// The most channels a file may have: what [`crate::convert`] knows the
/// layout of, eight.
pub const MAX_CHANNELS: u16 = 8;

/// The highest rate a file may have, in frames a second.
pub const MAX_RATE: u32 = 768_000;

/// `WAVE_FORMAT_PCM`.
const FORMAT_PCM: u16 = 1;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const FORMAT_FLOAT: u16 = 3;
/// `WAVE_FORMAT_EXTENSIBLE`: the real format is the sub-format GUID's first
/// two bytes.
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Whether `bytes` begin as a WAV file: a RIFF file of form `WAVE`.
#[must_use]
pub fn is_wav(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE")
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// How the samples are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    /// Integers of this many bytes: 1 (unsigned), 2, 3 or 4 (signed).
    Int(usize),
    /// Floating point of this many bytes: 4 or 8.
    Float(usize),
}

impl Encoding {
    fn bytes(self) -> usize {
        match self {
            Self::Int(n) | Self::Float(n) => n,
        }
    }
}

/// What the `fmt ` chunk says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
    channels: u16,
    rate: u32,
    encoding: Encoding,
}

/// The `fmt ` chunk's body, read and checked.
fn read_format(body: &[u8]) -> Result<Format, DecodeError> {
    let short = DecodeError::Wav("its format chunk is cut short");
    let mut tag = u16_at(body, 0).ok_or(short.clone())?;
    let channels = u16_at(body, 2).ok_or(short.clone())?;
    let rate = u32_at(body, 4).ok_or(short.clone())?;
    let block_align = u16_at(body, 12).ok_or(short.clone())?;
    let bits = u16_at(body, 14).ok_or(short.clone())?;
    if tag == FORMAT_EXTENSIBLE {
        // cbSize, wValidBitsPerSample, dwChannelMask, then the GUID, whose
        // first two bytes are the format.
        tag = u16_at(body, 24).ok_or(short)?;
    }
    let encoding = match (tag, bits) {
        (FORMAT_PCM, 8) => Encoding::Int(1),
        (FORMAT_PCM, 16) => Encoding::Int(2),
        (FORMAT_PCM, 24) => Encoding::Int(3),
        (FORMAT_PCM, 32) => Encoding::Int(4),
        (FORMAT_FLOAT, 32) => Encoding::Float(4),
        (FORMAT_FLOAT, 64) => Encoding::Float(8),
        (FORMAT_PCM | FORMAT_FLOAT, _) => {
            return Err(DecodeError::Wav(
                "its samples are a size this does not read",
            ));
        }
        _ => {
            return Err(DecodeError::Wav(
                "its samples are compressed in a way this does not read",
            ));
        }
    };
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(DecodeError::Wav("it has no channels, or more than eight"));
    }
    if rate == 0 || rate > MAX_RATE {
        return Err(DecodeError::Wav("its rate is nought or past 768 kHz"));
    }
    let frame = usize::from(channels).saturating_mul(encoding.bytes());
    if usize::from(block_align) != frame {
        return Err(DecodeError::Wav(
            "its frame size does not match its samples",
        ));
    }
    Ok(Format {
        channels,
        rate,
        encoding,
    })
}

/// One sample at `bytes`' start, 1.0 full scale.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "integer samples of at most 32 bits scaled to -1..1; an f64 sample narrowed to f32"
)]
fn sample(bytes: &[u8], encoding: Encoding) -> f32 {
    let value = match (encoding, bytes) {
        (Encoding::Int(1), [b, ..]) => (f32::from(*b) - 128.0) / 128.0,
        (Encoding::Int(2), [a, b, ..]) => f32::from(i16::from_le_bytes([*a, *b])) / 32_768.0,
        (Encoding::Int(3), [a, b, c, ..]) => {
            // Sign-extended from the top byte: an arithmetic shift.
            let v = i32::from_le_bytes([0, *a, *b, *c]).wrapping_shr(8);
            v as f32 / 8_388_608.0
        }
        (Encoding::Int(4), [a, b, c, d, ..]) => {
            i32::from_le_bytes([*a, *b, *c, *d]) as f32 / 2_147_483_648.0
        }
        (Encoding::Float(4), [a, b, c, d, ..]) => f32::from_le_bytes([*a, *b, *c, *d]),
        (Encoding::Float(8), [a, b, c, d, e, f, g, h, ..]) => {
            f64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h]) as f32
        }
        _ => 0.0,
    };
    if value.is_finite() { value } else { 0.0 }
}

/// The WAV file in `bytes`.
///
/// # Errors
///
/// [`DecodeError::Unrecognised`] for bytes that are not a WAV file, and
/// [`DecodeError::Wav`] for one this cannot read, saying why.
pub fn read(bytes: &[u8]) -> Result<Audio, DecodeError> {
    if !is_wav(bytes) {
        return Err(DecodeError::Unrecognised);
    }
    let mut at = 12usize;
    let mut format = None;
    while let (Some(id), Some(size)) = (
        bytes.get(at..at.saturating_add(4)),
        u32_at(bytes, at.saturating_add(4)),
    ) {
        let body_at = at.saturating_add(8);
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let remaining = bytes.len().saturating_sub(body_at);
        if id == b"data" {
            let format = format.ok_or(DecodeError::Wav("its samples come before their format"))?;
            // A size of nought or past the end is a streaming writer's:
            // the rest of the file is the data.
            let len = if size == 0 || size > remaining {
                remaining
            } else {
                size
            };
            let body = bytes
                .get(body_at..body_at.saturating_add(len))
                .unwrap_or(&[]);
            return Ok(decode_samples(body, format));
        }
        if size > remaining {
            return Err(DecodeError::Wav("a chunk claims more than the file holds"));
        }
        if id == b"fmt " {
            let body = bytes
                .get(body_at..body_at.saturating_add(size))
                .unwrap_or(&[]);
            format = Some(read_format(body)?);
        }
        // Chunks are padded to an even size.
        at = body_at.saturating_add(size).saturating_add(size & 1);
    }
    Err(DecodeError::Wav("it has no samples"))
}

/// `body`'s whole frames in `format`, up to [`MAX_SECONDS`] of them.
fn decode_samples(body: &[u8], format: Format) -> Audio {
    let width = format.encoding.bytes();
    let frame = usize::from(format.channels).saturating_mul(width);
    let most = usize::try_from(format.rate)
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(MAX_SECONDS).unwrap_or(usize::MAX));
    let samples = body
        .chunks_exact(frame.max(1))
        .take(most)
        .flat_map(|f| f.chunks_exact(width.max(1)))
        .map(|s| sample(s, format.encoding))
        .collect();
    Audio {
        rate: format.rate,
        channels: format.channels,
        samples,
        order: ChannelOrder::Wav,
    }
}

#[cfg(test)]
#[path = "wav_tests.rs"]
mod tests;
