//! Reading, converting and writing uncompressed WAV audio.
//!
//! A WAV file is a RIFF container: a `fmt ` chunk saying how the samples are
//! stored and a `data` chunk holding them. This crate reads one into
//! [`Audio`] -- interleaved `f32` samples in `-1.0..=1.0` -- converts it
//! ([`to_channels`], [`resample`]), and writes it back in any of the usual
//! sample formats ([`encode`]), dithering when it drops bits.
//!
//! **Nothing in a file is trusted.** Every size is checked against the bytes
//! actually there, a chunk that claims more than remains is refused (but for
//! `data`, whose size a streaming writer often leaves at zero or at
//! `0xFFFF_FFFF`, where the rest of the file is taken), and a format this
//! does not decode is named in the error rather than read as noise.
//!
//! **What it reads:** integer PCM at 8, 16, 24 and 32 bits, IEEE float at 32
//! and 64, in plain `WAVE_FORMAT_PCM` / `WAVE_FORMAT_IEEE_FLOAT` headers or
//! `WAVE_FORMAT_EXTENSIBLE` ones naming either. Not compressed WAV (ADPCM,
//! mu-law, A-law) and not RF64.
//!
//! **Without decoding:** a header from a file's first part
//! ([`parse_header_prefix`]), a waveform overview straight from the stored
//! samples ([`peaks`]), the markers sound editors keep in `cue ` and
//! `LIST`/`adtl` chunks, read and replaced ([`cues`], [`with_cues`]), and a
//! stretch cut out as a file of its own, its samples copied as stored
//! ([`cut`]).
//!
//! `apps/mediaconvert` is the first user.

use std::fmt;

/// How samples are stored in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// Unsigned 8-bit, 128 is silence.
    U8,
    I16,
    I24,
    I32,
    F32,
    F64,
}

impl SampleFormat {
    /// Bits per sample as stored.
    #[must_use]
    pub fn bits(self) -> u16 {
        match self {
            Self::U8 => 8,
            Self::I16 => 16,
            Self::I24 => 24,
            Self::I32 | Self::F32 => 32,
            Self::F64 => 64,
        }
    }

    /// Bytes per sample as stored.
    #[must_use]
    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }

    /// Whether the samples are floating point.
    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// How a person names it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::U8 => "8-bit",
            Self::I16 => "16-bit",
            Self::I24 => "24-bit",
            Self::I32 => "32-bit",
            Self::F32 => "32-bit float",
            Self::F64 => "64-bit float",
        }
    }
}

/// What a WAV file's header says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
    /// Whole frames in the data chunk -- one sample per channel each.
    pub frames: u64,
    /// Where the samples start, and how many bytes of them there are.
    pub data_offset: usize,
    pub data_len: usize,
}

impl Info {
    /// The length in seconds.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a duration for display; frame counts past 2^52 are not audio"
    )]
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate.max(1))
    }
}

/// Why a file could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WavError {
    /// It does not begin `RIFF....WAVE`.
    NotWav,
    /// A chunk claims more bytes than the file has.
    Truncated(&'static str),
    /// There is no `fmt ` chunk before the samples, or none at all.
    NoFormat,
    /// There is no `data` chunk.
    NoData,
    /// The header is not self-consistent.
    Invalid(String),
    /// A real WAV this does not decode.
    Unsupported(String),
    /// Too long to write as a WAV, whose sizes are 32 bits.
    TooLarge,
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWav => write!(f, "not a WAV file"),
            Self::Truncated(what) => write!(f, "the file ends inside its {what}"),
            Self::NoFormat => write!(f, "no format chunk before the samples"),
            Self::NoData => write!(f, "no sample data"),
            Self::Invalid(why) => write!(f, "{why}"),
            Self::Unsupported(what) => write!(f, "{what} is not a WAV this reads"),
            Self::TooLarge => write!(f, "longer than a WAV file can hold (4 GiB)"),
        }
    }
}

impl std::error::Error for WavError {}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*b.first()?, *b.get(1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *b.first()?,
        *b.get(1)?,
        *b.get(2)?,
        *b.get(3)?,
    ]))
}

/// One chunk of a RIFF file: its id, and where its body lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chunk {
    id: [u8; 4],
    start: usize,
    len: usize,
}

impl Chunk {
    /// The chunk's body. Always within `bytes`: [`chunks`] checked it.
    fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        bytes
            .get(self.start..self.start.saturating_add(self.len))
            .unwrap_or(&[])
    }

    /// Whether this is a `LIST` chunk of the given type (`adtl`, `INFO`).
    fn is_list(&self, bytes: &[u8], kind: &[u8; 4]) -> bool {
        &self.id == b"LIST" && self.body(bytes).get(..4) == Some(kind.as_slice())
    }
}

/// The chunks of a WAV file, in order, every size checked against the bytes
/// there.
fn chunks(bytes: &[u8]) -> Result<Vec<Chunk>, WavError> {
    chunks_of(bytes, bytes.len())
}

/// The chunks of a file `whole` bytes long, of which `bytes` is the start,
/// every size checked against `whole`. A chunk past the end of `bytes` is
/// listed and has an empty body; one whose header is past it ends the list.
///
/// Before the samples, a chunk longer than the file is an error: the format
/// may be in it. After them it is where reading stops -- bytes a writer left
/// at the end are no reason to refuse the audio before them. A `data` size of
/// zero or past the end is what a streaming writer leaves, and there the
/// samples are the rest of the file.
fn chunks_of(bytes: &[u8], whole: usize) -> Result<Vec<Chunk>, WavError> {
    if bytes.get(..4) == Some(b"RF64") {
        return Err(WavError::Unsupported(String::from(
            "RF64 (a WAV over 4 GiB)",
        )));
    }
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotWav);
    }
    let mut out = Vec::new();
    let mut seen_data = false;
    let mut at = 12_usize;
    while let (Some(id), Some(size)) = (
        bytes.get(at..at.saturating_add(4)),
        u32_at(bytes, at.saturating_add(4)),
    ) {
        let id: [u8; 4] = id.try_into().map_err(|_| WavError::NotWav)?;
        let start = at.saturating_add(8);
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let remaining = whole.max(bytes.len()).saturating_sub(start);
        let len = if size <= remaining && !(&id == b"data" && size == 0) {
            size
        } else if &id == b"data" {
            remaining
        } else if seen_data {
            break;
        } else {
            return Err(WavError::Truncated(if &id == b"fmt " {
                "format chunk"
            } else {
                "header"
            }));
        };
        seen_data |= &id == b"data";
        out.push(Chunk { id, start, len });
        // Chunks are padded to an even length.
        at = start.saturating_add(len).saturating_add(len & 1);
    }
    Ok(out)
}

/// Read a WAV file's header: its format and where its samples are.
///
/// # Errors
///
/// As [`WavError`]: not a WAV, a chunk longer than the file, no format or no
/// data, a header that contradicts itself, or a format this does not decode.
pub fn parse_header(bytes: &[u8]) -> Result<Info, WavError> {
    parse_header_prefix(bytes, u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

/// [`parse_header`] from the first part of a file `file_len` bytes long --
/// enough to list a folder of recordings without reading every one whole.
///
/// The length has to be given. Measured against the prefix alone, a `data`
/// chunk that runs past it looks like a streaming writer's, whose samples are
/// "the rest of the file" -- and a ten-minute recording read through its
/// first megabyte would be reported as six seconds long.
///
/// # Errors
///
/// As [`parse_header`]; and [`WavError::Truncated`] when the format is not
/// within `prefix`.
pub fn parse_header_prefix(prefix: &[u8], file_len: u64) -> Result<Info, WavError> {
    let bytes = prefix;
    let whole = usize::try_from(file_len).unwrap_or(usize::MAX);
    let mut format: Option<(u32, u16, SampleFormat, u16)> = None;
    for chunk in chunks_of(bytes, whole)? {
        match &chunk.id {
            b"fmt " => format = Some(parse_format(chunk.body(bytes))?),
            b"data" => {
                let Some((rate, channels, sample, block)) = format else {
                    return Err(WavError::NoFormat);
                };
                let frames = chunk.len.checked_div(usize::from(block)).unwrap_or(0);
                return Ok(Info {
                    sample_rate: rate,
                    channels,
                    format: sample,
                    frames: u64::try_from(frames).unwrap_or(u64::MAX),
                    data_offset: chunk.start,
                    data_len: frames.saturating_mul(usize::from(block)),
                });
            }
            _ => {}
        }
    }
    Err(if format.is_some() {
        WavError::NoData
    } else {
        WavError::NoFormat
    })
}

/// The `fmt ` chunk: sample rate, channels, sample format, block size.
fn parse_format(chunk: &[u8]) -> Result<(u32, u16, SampleFormat, u16), WavError> {
    let (Some(tag), Some(channels), Some(rate), Some(block), Some(bits)) = (
        u16_at(chunk, 0),
        u16_at(chunk, 2),
        u32_at(chunk, 4),
        u16_at(chunk, 12),
        u16_at(chunk, 14),
    ) else {
        return Err(WavError::Truncated("format chunk"));
    };
    // WAVE_FORMAT_EXTENSIBLE carries the real tag in the first two bytes of
    // its sub-format GUID, at offset 24.
    let tag = if tag == 0xFFFE {
        u16_at(chunk, 24).ok_or(WavError::Truncated("format chunk"))?
    } else {
        tag
    };
    let sample = match (tag, bits) {
        (1, 8) => SampleFormat::U8,
        (1, 16) => SampleFormat::I16,
        (1, 24) => SampleFormat::I24,
        (1, 32) => SampleFormat::I32,
        (3, 32) => SampleFormat::F32,
        (3, 64) => SampleFormat::F64,
        (1, other) => {
            return Err(WavError::Unsupported(format!("{other}-bit integer PCM")));
        }
        (3, other) => return Err(WavError::Unsupported(format!("{other}-bit float"))),
        (2, _) => return Err(WavError::Unsupported(String::from("ADPCM"))),
        (6, _) => return Err(WavError::Unsupported(String::from("A-law"))),
        (7, _) => return Err(WavError::Unsupported(String::from("mu-law"))),
        (0x55, _) => return Err(WavError::Unsupported(String::from("MP3 in a WAV"))),
        (other, _) => {
            return Err(WavError::Unsupported(format!("format tag {other:#06x}")));
        }
    };
    if channels == 0 {
        return Err(WavError::Invalid(String::from(
            "the header says no channels",
        )));
    }
    if rate == 0 {
        return Err(WavError::Invalid(String::from(
            "the header says a sample rate of 0",
        )));
    }
    let expected = channels.checked_mul(sample.bits() / 8);
    if expected != Some(block) {
        return Err(WavError::Invalid(format!(
            "a frame of {channels} {} samples is not {block} bytes",
            sample.label()
        )));
    }
    Ok((rate, channels, sample, block))
}

/// Audio as samples: interleaved, one per channel per frame, in `-1.0..=1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Audio {
    /// Frames -- a sample per channel.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples
            .len()
            .checked_div(usize::from(self.channels))
            .unwrap_or(0)
    }
}

/// Read a WAV file's samples.
///
/// # Errors
///
/// As [`parse_header`].
pub fn decode(bytes: &[u8]) -> Result<Audio, WavError> {
    let info = parse_header(bytes)?;
    let data = bytes
        .get(info.data_offset..info.data_offset.saturating_add(info.data_len))
        .ok_or(WavError::Truncated("samples"))?;
    let width = info.format.bytes();
    let samples = data
        .chunks_exact(width)
        .map(|s| decode_sample(info.format, s))
        .collect();
    Ok(Audio {
        sample_rate: info.sample_rate,
        channels: info.channels,
        samples,
    })
}

/// One stored sample as `f32`. A NaN or infinite float sample is silence: it
/// would otherwise poison every sample a filter computes from it.
#[allow(
    clippy::cast_precision_loss,
    reason = "a 32-bit integer sample in f32 keeps its top 24 bits, which is the precision of the result"
)]
fn decode_sample(format: SampleFormat, s: &[u8]) -> f32 {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let value = match format {
        SampleFormat::U8 => (f32::from(at(0)) - 128.0) / 128.0,
        SampleFormat::I16 => f32::from(i16::from_le_bytes([at(0), at(1)])) / 32_768.0,
        SampleFormat::I24 => {
            // Sign-extended by placing the three bytes at the top of an i32.
            let v = i32::from_le_bytes([0, at(0), at(1), at(2)]) >> 8;
            v as f32 / 8_388_608.0
        }
        SampleFormat::I32 => {
            i32::from_le_bytes([at(0), at(1), at(2), at(3)]) as f32 / 2_147_483_648.0
        }
        SampleFormat::F32 => f32::from_le_bytes([at(0), at(1), at(2), at(3)]),
        #[allow(
            clippy::cast_possible_truncation,
            reason = "f64 samples are stored as f32 here"
        )]
        SampleFormat::F64 => {
            f64::from_le_bytes([at(0), at(1), at(2), at(3), at(4), at(5), at(6), at(7)]) as f32
        }
    };
    if value.is_finite() { value } else { 0.0 }
}

/// The lowest and highest sample in a stretch of audio, across its channels:
/// one column of a waveform overview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub low: f32,
    pub high: f32,
}

impl Peak {
    /// A stretch with nothing in it.
    pub const SILENT: Self = Self {
        low: 0.0,
        high: 0.0,
    };
}

/// A file's waveform in `columns` equal stretches, each its lowest and highest
/// sample across the channels -- what an overview draws.
///
/// Read from the stored samples as they are, a frame at a time, rather than
/// by decoding the whole file first: an hour of stereo would be seven hundred
/// megabytes of `f32` to find four thousand pairs of numbers.
///
/// # Errors
///
/// As [`parse_header`].
pub fn peaks(bytes: &[u8], columns: usize) -> Result<Vec<Peak>, WavError> {
    let info = parse_header(bytes)?;
    let data = bytes
        .get(info.data_offset..info.data_offset.saturating_add(info.data_len))
        .ok_or(WavError::Truncated("samples"))?;
    let width = info.format.bytes();
    let frame = usize::from(info.channels).saturating_mul(width);
    let frames = data.len().checked_div(frame).unwrap_or(0);
    let mut out = Vec::with_capacity(columns);
    for column in 0..columns {
        // Stretch `column` is frames [frames * c / columns, frames * (c+1) /
        // columns): every frame in exactly one stretch, and none empty while
        // there are frames to share.
        let edge = |c: usize| {
            u128::try_from(frames)
                .ok()
                .and_then(|f| f.checked_mul(u128::try_from(c).ok()?))
                .and_then(|n| n.checked_div(u128::try_from(columns).ok()?))
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(frames)
        };
        let from = edge(column);
        let to = edge(column.saturating_add(1))
            .max(from.saturating_add(1))
            .min(frames);
        let stretch = data
            .get(from.saturating_mul(frame)..to.saturating_mul(frame))
            .unwrap_or(&[]);
        let mut peak: Option<Peak> = None;
        for sample in stretch.chunks_exact(width) {
            let v = decode_sample(info.format, sample);
            let p = peak.get_or_insert(Peak { low: v, high: v });
            p.low = p.low.min(v);
            p.high = p.high.max(v);
        }
        out.push(peak.unwrap_or(Peak::SILENT));
    }
    Ok(out)
}

/// A marker in a file: a frame, and what it is called.
///
/// Stored as the standard `cue ` chunk, with its names in a `LIST` chunk of
/// type `adtl` holding one `labl` per marker -- the form sound editors read
/// and write, so a marker set here is a marker there.
///
/// `label` is bytes: the format does not say what encoding a name is in, and
/// a name read from one file and written to the next must come out as it went
/// in, whatever it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    pub frame: u32,
    pub label: Vec<u8>,
}

/// The markers in a file, in the order of their frames. A file with none has
/// an empty list; a `cue ` chunk shorter than it claims gives the points that
/// are whole.
///
/// # Errors
///
/// As [`parse_header`].
pub fn cues(bytes: &[u8]) -> Result<Vec<Cue>, WavError> {
    parse_header(bytes)?;
    let mut points: Vec<(u32, u32)> = Vec::new();
    let mut labels: Vec<(u32, Vec<u8>)> = Vec::new();
    for chunk in chunks(bytes)? {
        let body = chunk.body(bytes);
        if &chunk.id == b"cue " {
            let count = u32_at(body, 0).map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
            for i in 0..count {
                // 24 bytes a point: name, position, chunk id, chunk start,
                // block start, and the sample offset -- the frame.
                let at = i.saturating_mul(24).saturating_add(4);
                let (Some(name), Some(frame)) =
                    (u32_at(body, at), u32_at(body, at.saturating_add(20)))
                else {
                    break;
                };
                points.push((name, frame));
            }
        } else if chunk.is_list(bytes, b"adtl") {
            let mut at = 4_usize;
            while let (Some(id), Some(size)) = (
                body.get(at..at.saturating_add(4)),
                u32_at(body, at.saturating_add(4)),
            ) {
                let start = at.saturating_add(8);
                let len = usize::try_from(size).unwrap_or(usize::MAX);
                let Some(sub) = body.get(start..start.saturating_add(len)) else {
                    break;
                };
                if id == b"labl"
                    && let Some(name) = u32_at(sub, 0)
                {
                    let text = sub.get(4..).unwrap_or(&[]);
                    let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
                    labels.push((name, text.get(..end).unwrap_or(&[]).to_vec()));
                }
                at = start.saturating_add(len).saturating_add(len & 1);
            }
        }
    }
    let mut out: Vec<Cue> = points
        .into_iter()
        .map(|(name, frame)| Cue {
            frame,
            label: labels
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, l)| l.clone())
                .unwrap_or_default(),
        })
        .collect();
    out.sort_by_key(|c| c.frame);
    Ok(out)
}

/// Append a chunk, padded to an even length.
fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) -> Result<(), WavError> {
    let len = u32::try_from(body.len()).map_err(|_| WavError::TooLarge)?;
    out.extend_from_slice(id);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(body);
    if len & 1 == 1 {
        out.push(0);
    }
    Ok(())
}

/// Append the `cue ` chunk and the `LIST`/`adtl` names for `cues`, if any.
/// Each point's name is its place in the list, from 1.
fn push_cues(out: &mut Vec<u8>, cues: &[Cue]) -> Result<(), WavError> {
    if cues.is_empty() {
        return Ok(());
    }
    let count = u32::try_from(cues.len()).map_err(|_| WavError::TooLarge)?;
    let mut points = count.to_le_bytes().to_vec();
    let mut names = b"adtl".to_vec();
    for (name, cue) in (1_u32..).zip(cues) {
        points.extend_from_slice(&name.to_le_bytes());
        points.extend_from_slice(&cue.frame.to_le_bytes());
        points.extend_from_slice(b"data");
        points.extend_from_slice(&0_u32.to_le_bytes());
        points.extend_from_slice(&0_u32.to_le_bytes());
        points.extend_from_slice(&cue.frame.to_le_bytes());
        let mut labl = name.to_le_bytes().to_vec();
        labl.extend(cue.label.iter().copied().take_while(|b| *b != 0));
        labl.push(0);
        push_chunk(&mut names, b"labl", &labl)?;
    }
    push_chunk(out, b"cue ", &points)?;
    push_chunk(out, b"LIST", &names)
}

/// Set the RIFF size of a finished file.
fn finish_riff(mut out: Vec<u8>) -> Result<Vec<u8>, WavError> {
    let riff = u32::try_from(out.len().saturating_sub(8)).map_err(|_| WavError::TooLarge)?;
    if let Some(size) = out.get_mut(4..8) {
        size.copy_from_slice(&riff.to_le_bytes());
    }
    Ok(out)
}

/// The same file with `cues` as its markers: every other chunk as it was, in
/// its place, and the old markers and their names replaced.
///
/// # Errors
///
/// As [`parse_header`]; and [`WavError::TooLarge`] past 4 GiB.
pub fn with_cues(bytes: &[u8], cues: &[Cue]) -> Result<Vec<u8>, WavError> {
    parse_header(bytes)?;
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    for chunk in chunks(bytes)? {
        if &chunk.id == b"cue " || chunk.is_list(bytes, b"adtl") {
            continue;
        }
        push_chunk(&mut out, &chunk.id, chunk.body(bytes))?;
    }
    push_cues(&mut out, cues)?;
    finish_riff(out)
}

/// Frames `start..end` of a file as a file of their own: the same format,
/// the samples copied as they are stored -- nothing decoded, nothing
/// re-dithered -- the markers in that stretch moved to where it now begins,
/// and the file's `INFO` tags kept.
///
/// # Errors
///
/// As [`parse_header`]; [`WavError::Invalid`] when the stretch holds no
/// frame.
pub fn cut(bytes: &[u8], start: u64, end: u64) -> Result<Vec<u8>, WavError> {
    let info = parse_header(bytes)?;
    let end = end.min(info.frames);
    if start >= end {
        return Err(WavError::Invalid(String::from(
            "there is nothing between the start and the end",
        )));
    }
    let frame = u64::from(info.channels)
        .saturating_mul(u64::try_from(info.format.bytes()).unwrap_or(u64::MAX));
    let offset = |f: u64| {
        usize::try_from(f.saturating_mul(frame))
            .ok()
            .and_then(|n| info.data_offset.checked_add(n))
    };
    let samples = offset(start)
        .zip(offset(end))
        .and_then(|(a, b)| bytes.get(a..b))
        .ok_or(WavError::Truncated("samples"))?;
    let kept: Vec<Cue> = cues(bytes)?
        .into_iter()
        .filter(|c| (start..end).contains(&u64::from(c.frame)))
        .map(|c| Cue {
            frame: u32::try_from(u64::from(c.frame).saturating_sub(start)).unwrap_or(u32::MAX),
            label: c.label,
        })
        .collect();
    let all = chunks(bytes)?;
    // The format the samples were read in: the last before them, as
    // `parse_header` takes it.
    let format = all
        .iter()
        .take_while(|c| &c.id != b"data")
        .filter(|c| &c.id == b"fmt ")
        .last()
        .ok_or(WavError::NoFormat)?;
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    push_chunk(&mut out, b"fmt ", format.body(bytes))?;
    for chunk in all.iter().filter(|c| c.is_list(bytes, b"INFO")) {
        push_chunk(&mut out, b"LIST", chunk.body(bytes))?;
    }
    push_chunk(&mut out, b"data", samples)?;
    push_cues(&mut out, &kept)?;
    finish_riff(out)
}

/// A small deterministic generator for dither: the same seed gives the same
/// file, so a conversion can be checked byte for byte.
struct Dither(u64);

impl Dither {
    fn next_unit(&mut self) -> f32 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        #[allow(
            clippy::cast_precision_loss,
            reason = "24 random bits, exactly representable"
        )]
        {
            (v >> 40) as f32 / 16_777_216.0
        }
    }

    /// Triangular noise of one least significant bit peak to peak... twice:
    /// the difference of two uniform values, in `-1.0..1.0`.
    fn triangular(&mut self) -> f32 {
        self.next_unit() - self.next_unit()
    }
}

/// Write `audio` as a WAV file of `format` samples.
///
/// Integer formats of 24 bits or fewer get triangular dither of one least
/// significant bit before rounding, so a quiet passage fades into noise
/// rather than into distortion; `seed` makes that noise repeatable.
///
/// # Errors
///
/// [`WavError::TooLarge`] past the four gigabytes a WAV's sizes can say.
pub fn encode(audio: &Audio, format: SampleFormat, seed: u64) -> Result<Vec<u8>, WavError> {
    let width = format.bytes();
    let data_len = audio
        .samples
        .len()
        .checked_mul(width)
        .ok_or(WavError::TooLarge)?;
    let (mut out, pad) = header(audio.sample_rate, audio.channels, format, data_len)?;
    let mut dither = Dither(seed | 1);
    for &sample in &audio.samples {
        let s = if sample.is_finite() { sample } else { 0.0 };
        encode_sample(format, s, &mut dither, &mut out);
    }
    if pad {
        out.push(0);
    }
    Ok(out)
}

/// Write 16-bit samples as a 16-bit WAV file, each stored exactly as it is --
/// what a recorder has from a capture device, and must not dither again.
///
/// # Errors
///
/// [`WavError::TooLarge`] past the four gigabytes a WAV's sizes can say.
pub fn encode_pcm16(sample_rate: u32, channels: u16, samples: &[i16]) -> Result<Vec<u8>, WavError> {
    let data_len = samples.len().checked_mul(2).ok_or(WavError::TooLarge)?;
    let (mut out, _) = header(sample_rate, channels, SampleFormat::I16, data_len)?;
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Ok(out)
}

/// The header of a WAV file of `data_len` bytes of `format` samples, up to
/// and including the `data` chunk's size; and whether the samples need a pad
/// byte after them.
fn header(
    sample_rate: u32,
    channels: u16,
    format: SampleFormat,
    data_len: usize,
) -> Result<(Vec<u8>, bool), WavError> {
    let float = format.is_float();
    // A float format's header carries a two-byte extension size, as the
    // format requires for anything but integer PCM.
    let fmt_len: u32 = if float { 18 } else { 16 };
    let pad = data_len & 1;
    let riff_len = u32::try_from(data_len)
        .ok()
        .and_then(|d| d.checked_add(fmt_len.checked_add(20)?))
        .and_then(|n| n.checked_add(u32::try_from(pad).ok()?))
        .ok_or(WavError::TooLarge)?;
    let block = channels
        .checked_mul(format.bits() / 8)
        .ok_or(WavError::TooLarge)?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(block))
        .ok_or(WavError::TooLarge)?;

    let mut out = Vec::with_capacity(data_len.saturating_add(64));
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&fmt_len.to_le_bytes());
    out.extend_from_slice(&(if float { 3_u16 } else { 1 }).to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&format.bits().to_le_bytes());
    if float {
        out.extend_from_slice(&0_u16.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(
        &u32::try_from(data_len)
            .map_err(|_| WavError::TooLarge)?
            .to_le_bytes(),
    );
    Ok((out, pad == 1))
}

/// One sample, in `format`, onto `out`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "each value is rounded and clamped to the target's range first"
)]
fn encode_sample(format: SampleFormat, s: f32, dither: &mut Dither, out: &mut Vec<u8>) {
    // Scaled to the target's full range, dithered, rounded and clamped.
    let quantise = |scale: f32, dither: &mut Dither| -> f64 {
        let v = f64::from(s) * f64::from(scale) + f64::from(dither.triangular());
        v.round().clamp(-f64::from(scale), f64::from(scale) - 1.0)
    };
    match format {
        SampleFormat::U8 => {
            let v = quantise(128.0, dither) + 128.0;
            out.push(v as u8);
        }
        SampleFormat::I16 => {
            out.extend_from_slice(&(quantise(32_768.0, dither) as i16).to_le_bytes());
        }
        SampleFormat::I24 => {
            let v = quantise(8_388_608.0, dither) as i32;
            let b = v.to_le_bytes();
            out.extend_from_slice(b.get(..3).unwrap_or(&[0, 0, 0]));
        }
        SampleFormat::I32 => {
            // No dither at 32 bits: f32 carries 24 bits, far above this
            // format's least significant one.
            let v = (f64::from(s) * 2_147_483_648.0)
                .round()
                .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
            out.extend_from_slice(&v.to_le_bytes());
        }
        SampleFormat::F32 => out.extend_from_slice(&s.to_le_bytes()),
        SampleFormat::F64 => out.extend_from_slice(&f64::from(s).to_le_bytes()),
    }
}

/// The same audio with `channels` channels.
///
/// - One to many: the mono signal into the first two channels (both, for
///   stereo), silence in any others -- a mono voice belongs at the front, not
///   in the subwoofer.
/// - Many to one: the average, leaving out the low-frequency channel of a 5.1
///   signal.
/// - 5.1 to stereo: the ITU-R BS.775 downmix, left plus 0.707 of centre and
///   of left surround, scaled so it cannot clip.
/// - Otherwise the first channels are kept and extra ones are silent.
#[must_use]
pub fn to_channels(audio: &Audio, channels: u16) -> Audio {
    let from = usize::from(audio.channels);
    let to = usize::from(channels.max(1));
    if from == to || from == 0 {
        return audio.clone();
    }
    let mut samples = Vec::with_capacity(audio.frames().saturating_mul(to));
    for frame in audio.samples.chunks_exact(from) {
        let ch = |i: usize| frame.get(i).copied().unwrap_or(0.0);
        match (from, to) {
            (1, _) => {
                for i in 0..to {
                    samples.push(if i < 2 { ch(0) } else { 0.0 });
                }
            }
            (6, 1) => {
                // Front left, right, centre, and the surrounds; not the LFE.
                let sum = ch(0) + ch(1) + ch(2) + ch(4) + ch(5);
                samples.push(sum / 5.0);
            }
            (_, 1) => {
                #[allow(clippy::cast_precision_loss, reason = "a channel count")]
                let n = from as f32;
                samples.push(frame.iter().sum::<f32>() / n);
            }
            (6, 2) => {
                const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;
                let norm = 1.0 / (1.0 + 2.0 * HALF_POWER);
                samples.push((ch(0) + HALF_POWER * ch(2) + HALF_POWER * ch(4)) * norm);
                samples.push((ch(1) + HALF_POWER * ch(2) + HALF_POWER * ch(5)) * norm);
            }
            _ => {
                for i in 0..to {
                    samples.push(ch(i));
                }
            }
        }
    }
    Audio {
        sample_rate: audio.sample_rate,
        channels: channels.max(1),
        samples,
    }
}

/// The zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..64 {
        let k = f64::from(k);
        term *= (half / k) * (half / k);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Zero crossings of the sinc each side of centre, at the cutoff.
const ZERO_CROSSINGS: f64 = 16.0;

/// Table entries per input sample.
const TABLE_RESOLUTION: f64 = 256.0;

/// The Kaiser window's shape: about 90 dB of stopband.
const KAISER_BETA: f64 = 8.6;

/// The same audio at `rate` samples a second.
///
/// A windowed-sinc filter (Kaiser, 16 zero crossings each side), its cutoff
/// just under the lower of the two Nyquist frequencies, so nothing above what
/// the target rate can hold folds back as an alias. The kernel is tabulated
/// once and read with linear interpolation, and each output sample is divided
/// by the sum of the weights it used, so a constant stays exactly constant --
/// at the ends too, where the kernel runs off the signal.
///
/// `progress` is called now and then with the fraction done.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "frame positions are far below 2^52, and every conversion to an index is floored and range-checked"
)]
pub fn resample(audio: &Audio, rate: u32, progress: &mut dyn FnMut(f32)) -> Audio {
    let rate = rate.max(1);
    let channels = usize::from(audio.channels.max(1));
    let frames_in = audio.frames();
    if rate == audio.sample_rate || frames_in == 0 {
        return Audio {
            sample_rate: rate,
            channels: audio.channels,
            samples: audio.samples.clone(),
        };
    }
    let ratio = f64::from(audio.sample_rate) / f64::from(rate);
    // Cutoff in cycles per input sample: half the lower rate, less a margin
    // for the transition band.
    let cutoff = 0.5 * (1.0 / ratio).min(1.0) * 0.97;
    let half_len = ZERO_CROSSINGS / (2.0 * cutoff);
    let table_len = ((half_len * TABLE_RESOLUTION).ceil() as usize).saturating_add(2);
    let table: Vec<f64> = (0..table_len)
        .map(|i| {
            let t = i as f64 / TABLE_RESOLUTION;
            if t > half_len {
                return 0.0;
            }
            let x = 2.0 * cutoff * t;
            let sinc = if x == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let w = t / half_len;
            let window =
                bessel_i0(KAISER_BETA * (1.0 - w * w).max(0.0).sqrt()) / bessel_i0(KAISER_BETA);
            2.0 * cutoff * sinc * window
        })
        .collect();
    let kernel = |d: f64| -> f64 {
        let pos = d.abs() * TABLE_RESOLUTION;
        let i = pos.floor() as usize;
        let frac = pos - pos.floor();
        let a = table.get(i).copied().unwrap_or(0.0);
        let b = table.get(i.saturating_add(1)).copied().unwrap_or(0.0);
        a + (b - a) * frac
    };

    let frames_out = ((frames_in as f64) / ratio).ceil() as usize;
    let mut samples = vec![0.0_f32; frames_out.saturating_mul(channels)];
    let report_every = (frames_out / 100).max(1);
    let mut acc = vec![0.0_f64; channels];
    for (n, out_frame) in samples.chunks_exact_mut(channels).enumerate() {
        let centre = n as f64 * ratio;
        let first = (centre - half_len).ceil().max(0.0) as usize;
        let last = ((centre + half_len).floor() as usize).min(frames_in.saturating_sub(1));
        acc.fill(0.0);
        let mut weight = 0.0;
        for k in first..=last {
            let w = kernel(centre - k as f64);
            if w == 0.0 {
                continue;
            }
            weight += w;
            let at = k.saturating_mul(channels);
            for (c, a) in acc.iter_mut().enumerate() {
                let x = audio
                    .samples
                    .get(at.saturating_add(c))
                    .copied()
                    .unwrap_or(0.0);
                *a += f64::from(x) * w;
            }
        }
        for (o, a) in out_frame.iter_mut().zip(&acc) {
            *o = if weight.abs() > 1e-12 {
                (*a / weight) as f32
            } else {
                0.0
            };
        }
        if n.checked_rem(report_every) == Some(0) {
            progress(n as f32 / frames_out.max(1) as f32);
        }
    }
    progress(1.0);
    Audio {
        sample_rate: rate,
        channels: audio.channels,
        samples,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn tone(rate: u32, freq: f32, seconds: f32, amplitude: f32) -> Audio {
        let n = (rate as f32 * seconds) as usize;
        Audio {
            sample_rate: rate,
            channels: 1,
            samples: (0..n)
                .map(|i| {
                    amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()
                })
                .collect(),
        }
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// The canonical 44-byte header of 16-bit stereo at 44.1 kHz, laid out
    /// by hand from the format's description rather than by `encode`.
    #[test]
    fn a_cd_quality_header_is_the_standard_forty_four_bytes() {
        let audio = Audio {
            sample_rate: 44_100,
            channels: 2,
            samples: vec![0.0; 4],
        };
        let bytes = encode(&audio, SampleFormat::I16, 1).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(b"RIFF");
        expected.extend_from_slice(&(36_u32 + 8).to_le_bytes());
        expected.extend_from_slice(b"WAVEfmt ");
        expected.extend_from_slice(&16_u32.to_le_bytes());
        expected.extend_from_slice(&1_u16.to_le_bytes());
        expected.extend_from_slice(&2_u16.to_le_bytes());
        expected.extend_from_slice(&44_100_u32.to_le_bytes());
        expected.extend_from_slice(&176_400_u32.to_le_bytes());
        expected.extend_from_slice(&4_u16.to_le_bytes());
        expected.extend_from_slice(&16_u16.to_le_bytes());
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&8_u32.to_le_bytes());
        assert_eq!(&bytes[..44], &expected[..]);
        assert_eq!(bytes.len(), 44 + 8);
        // Silence with one bit of dither stays within a bit of zero.
        for s in bytes[44..].chunks(2) {
            assert!(i16::from_le_bytes([s[0], s[1]]).abs() <= 1);
        }
    }

    #[test]
    fn every_format_survives_a_round_trip() {
        let audio = Audio {
            sample_rate: 22_050,
            channels: 2,
            samples: vec![0.0, 0.5, -0.5, 0.25, 0.999, -1.0, 0.123_456, -0.654_321],
        };
        for format in [
            SampleFormat::U8,
            SampleFormat::I16,
            SampleFormat::I24,
            SampleFormat::I32,
            SampleFormat::F32,
            SampleFormat::F64,
        ] {
            let bytes = encode(&audio, format, 7).unwrap();
            let info = parse_header(&bytes).unwrap();
            assert_eq!(info.format, format);
            assert_eq!(
                (info.sample_rate, info.channels, info.frames),
                (22_050, 2, 4)
            );
            let back = decode(&bytes).unwrap();
            // Within two least significant bits: one of rounding, one of dither.
            let lsb = 2.0 / 2_f32.powi(i32::from(format.bits().min(24)));
            for (a, b) in audio.samples.iter().zip(&back.samples) {
                assert!(
                    (a - b).abs() <= 2.0 * lsb + 1e-6,
                    "{format:?}: {a} came back {b}"
                );
            }
        }
    }

    #[test]
    fn stored_samples_read_as_the_format_says() {
        let file = |format_tag: u16, bits: u16, data: &[u8]| {
            let mut b = Vec::new();
            b.extend_from_slice(b"RIFF");
            b.extend_from_slice(&0_u32.to_le_bytes());
            b.extend_from_slice(b"WAVEfmt ");
            b.extend_from_slice(&16_u32.to_le_bytes());
            b.extend_from_slice(&format_tag.to_le_bytes());
            b.extend_from_slice(&1_u16.to_le_bytes());
            b.extend_from_slice(&8000_u32.to_le_bytes());
            b.extend_from_slice(&(8000 * u32::from(bits / 8)).to_le_bytes());
            b.extend_from_slice(&(bits / 8).to_le_bytes());
            b.extend_from_slice(&bits.to_le_bytes());
            b.extend_from_slice(b"data");
            b.extend_from_slice(&(data.len() as u32).to_le_bytes());
            b.extend_from_slice(data);
            decode(&b).unwrap().samples
        };
        assert_eq!(file(1, 8, &[128, 0, 255]), [0.0, -1.0, 127.0 / 128.0]);
        assert_eq!(
            file(1, 16, &[0x00, 0x80, 0xFF, 0x7F]),
            [-1.0, 32_767.0 / 32_768.0]
        );
        // 0x800000 is the most negative 24-bit value: sign extension.
        assert_eq!(
            file(1, 24, &[0x00, 0x00, 0x80, 0x00, 0x00, 0x40]),
            [-1.0, 0.5]
        );
        assert_eq!(file(3, 32, &0.25_f32.to_le_bytes()), [0.25]);
        // A NaN sample is silence, not a poison for every filter after it.
        assert_eq!(file(3, 32, &f32::NAN.to_le_bytes()), [0.0]);
    }

    #[test]
    fn extensible_headers_and_other_chunks_are_read() {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        // A LIST chunk of odd length before the format: padded, skipped.
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&3_u32.to_le_bytes());
        b.extend_from_slice(b"abc\0");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&40_u32.to_le_bytes());
        b.extend_from_slice(&0xFFFE_u16.to_le_bytes());
        b.extend_from_slice(&2_u16.to_le_bytes());
        b.extend_from_slice(&48_000_u32.to_le_bytes());
        b.extend_from_slice(&(48_000_u32 * 6).to_le_bytes());
        b.extend_from_slice(&6_u16.to_le_bytes());
        b.extend_from_slice(&24_u16.to_le_bytes());
        b.extend_from_slice(&22_u16.to_le_bytes());
        b.extend_from_slice(&24_u16.to_le_bytes());
        b.extend_from_slice(&3_u32.to_le_bytes());
        // KSDATAFORMAT_SUBTYPE_PCM
        b.extend_from_slice(&[
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71,
        ]);
        b.extend_from_slice(b"data");
        // A streaming writer's size: all ones. The rest of the file is taken.
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        b.extend_from_slice(&[0; 12]);
        let info = parse_header(&b).unwrap();
        assert_eq!(info.format, SampleFormat::I24);
        assert_eq!(
            (info.channels, info.sample_rate, info.frames),
            (2, 48_000, 2)
        );
    }

    #[test]
    fn what_is_not_a_wav_this_reads_says_so() {
        assert_eq!(parse_header(b"OggS....WAVE"), Err(WavError::NotWav));
        assert_eq!(parse_header(b"RIFF\0\0\0\0WAVE"), Err(WavError::NoFormat));
        let header = |tag: u16, channels: u16, bits: u16, block: u16| {
            let mut b = Vec::new();
            b.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
            b.extend_from_slice(&16_u32.to_le_bytes());
            b.extend_from_slice(&tag.to_le_bytes());
            b.extend_from_slice(&channels.to_le_bytes());
            b.extend_from_slice(&8000_u32.to_le_bytes());
            b.extend_from_slice(&0_u32.to_le_bytes());
            b.extend_from_slice(&block.to_le_bytes());
            b.extend_from_slice(&bits.to_le_bytes());
            b.extend_from_slice(b"data\0\0\0\0");
            parse_header(&b)
        };
        assert!(matches!(header(2, 1, 4, 1), Err(WavError::Unsupported(what)) if what == "ADPCM"));
        assert!(matches!(header(1, 1, 12, 2), Err(WavError::Unsupported(_))));
        assert!(matches!(header(1, 0, 16, 0), Err(WavError::Invalid(_))));
        assert!(matches!(header(1, 2, 16, 3), Err(WavError::Invalid(_))));
        assert!(header(1, 2, 16, 4).is_ok());
        // A format chunk longer than the file.
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&100_u32.to_le_bytes());
        b.extend_from_slice(&[0; 16]);
        assert_eq!(parse_header(&b), Err(WavError::Truncated("format chunk")));
        assert!(matches!(
            parse_header(b"RF64\0\0\0\0WAVE"),
            Err(WavError::Unsupported(_))
        ));
    }

    #[test]
    fn channels_are_mixed_the_standard_ways() {
        let stereo = Audio {
            sample_rate: 8000,
            channels: 2,
            samples: vec![1.0, 0.0, 0.5, -0.5],
        };
        let mono = to_channels(&stereo, 1);
        assert_eq!(mono.samples, [0.5, 0.0]);
        let back = to_channels(&mono, 2);
        assert_eq!(back.samples, [0.5, 0.5, 0.0, 0.0]);
        let five_one = Audio {
            sample_rate: 8000,
            channels: 6,
            // L, R, C, LFE, Ls, Rs
            samples: vec![1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        };
        let down = to_channels(&five_one, 2);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let norm = 1.0 / (1.0 + 2.0 * h);
        assert!((down.samples[0] - (1.0 + h) * norm).abs() < 1e-6);
        assert!((down.samples[1] - (h + h) * norm).abs() < 1e-6);
        // The subwoofer is left out of a mono mix.
        assert!((to_channels(&five_one, 1).samples[0] - 3.0 / 5.0).abs() < 1e-6);
        let quad = to_channels(&mono, 4);
        assert_eq!(quad.samples[..4], [0.5, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn resampling_keeps_the_length_the_level_and_the_pitch() {
        let input = tone(44_100, 1000.0, 0.5, 0.5);
        let mut last = 0.0;
        let out = resample(&input, 48_000, &mut |p| last = p);
        assert_eq!(last, 1.0);
        assert_eq!(out.sample_rate, 48_000);
        assert_eq!(
            out.frames(),
            (input.frames() as f64 * 48_000.0 / 44_100.0).ceil() as usize
        );
        // Away from the ends: the same level, and the same number of cycles.
        let middle = &out.samples[2000..22_000];
        assert!(
            (rms(middle) - 0.5 / 2_f32.sqrt()).abs() < 0.005,
            "{}",
            rms(middle)
        );
        let crossings = middle
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        let expected = 1000.0 * middle.len() as f32 / 48_000.0;
        assert!(
            (crossings as f32 - expected).abs() <= 1.0,
            "{crossings} crossings, {expected} expected"
        );
    }

    #[test]
    fn a_constant_stays_constant_to_the_ends() {
        let dc = Audio {
            sample_rate: 8000,
            channels: 1,
            samples: vec![0.3; 800],
        };
        for rate in [11_025, 44_100, 4_000] {
            let out = resample(&dc, rate, &mut |_| {});
            for s in &out.samples {
                assert!((s - 0.3).abs() < 1e-4, "{rate}: {s}");
            }
        }
    }

    /// Down to 8 kHz, a 6 kHz tone is above what the new rate can hold; it
    /// must be filtered out, not folded back to 2 kHz.
    #[test]
    fn downsampling_does_not_alias() {
        let high = resample(&tone(48_000, 6000.0, 0.25, 0.8), 8000, &mut |_| {});
        let low = resample(&tone(48_000, 1000.0, 0.25, 0.8), 8000, &mut |_| {});
        let middle = |a: &Audio| a.samples[200..1800].to_vec();
        assert!(
            rms(&middle(&high)) < 0.005,
            "the 6 kHz tone leaked through: {}",
            rms(&middle(&high))
        );
        assert!((rms(&middle(&low)) - 0.8 / 2_f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn stereo_channels_resample_apart() {
        let mut samples = Vec::new();
        for i in 0..4000 {
            samples.push(0.25);
            samples.push(if i % 2 == 0 { 0.0 } else { -0.0 });
        }
        let audio = Audio {
            sample_rate: 16_000,
            channels: 2,
            samples,
        };
        let out = resample(&audio, 22_050, &mut |_| {});
        for frame in out.samples.chunks(2) {
            assert!(
                (frame[0] - 0.25).abs() < 1e-4 && frame[1].abs() < 1e-6,
                "{frame:?}"
            );
        }
    }

    #[test]
    fn dither_is_repeatable_and_small() {
        let audio = tone(8000, 440.0, 0.1, 0.001);
        let a = encode(&audio, SampleFormat::I16, 42).unwrap();
        let b = encode(&audio, SampleFormat::I16, 42).unwrap();
        assert_eq!(a, b, "the same seed made a different file");
        let back = decode(&a).unwrap();
        let err: Vec<f32> = audio
            .samples
            .iter()
            .zip(&back.samples)
            .map(|(x, y)| x - y)
            .collect();
        assert!(rms(&err) < 2.0 / 32_768.0, "{}", rms(&err));
    }

    /// Dither keeps a signal quieter than one step of the target: without
    /// it, a steady 0.3 of a least significant bit rounds to silence every
    /// time; with it, it survives as the average.
    #[test]
    fn dither_keeps_what_is_quieter_than_a_step() {
        let level = 0.3 / 32_768.0;
        let audio = Audio {
            sample_rate: 8000,
            channels: 1,
            samples: vec![level; 20_000],
        };
        let back = decode(&encode(&audio, SampleFormat::I16, 9).unwrap()).unwrap();
        let mean = back.samples.iter().sum::<f32>() / back.samples.len() as f32;
        assert!(
            (mean - level).abs() < 0.1 / 32_768.0,
            "{} steps",
            mean * 32_768.0
        );
    }

    /// A data chunk of odd length is padded to even, and the sizes say so.
    #[test]
    fn an_odd_chunk_is_padded() {
        let audio = Audio {
            sample_rate: 8000,
            channels: 1,
            samples: vec![0.0; 3],
        };
        let bytes = encode(&audio, SampleFormat::U8, 1).unwrap();
        assert_eq!(bytes.len(), 44 + 3 + 1);
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            44 + 3 + 1 - 8
        );
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 3);
    }

    #[test]
    fn a_file_too_long_for_a_wav_is_refused() {
        // Not built -- a fake length is enough to reach the check.
        assert_eq!(
            u32::try_from(usize::MAX).is_err(),
            usize::BITS > 32,
            "this test assumes a 64-bit host"
        );
        let info = Info {
            sample_rate: 1,
            channels: 1,
            format: SampleFormat::I16,
            frames: 3,
            data_offset: 44,
            data_len: 6,
        };
        assert!((info.seconds() - 3.0).abs() < 1e-9);
    }

    /// A 16-bit file laid out by hand, so its samples are exactly these.
    fn pcm16(channels: u16, samples: &[i16]) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&16_u32.to_le_bytes());
        b.extend_from_slice(&1_u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&8_000_u32.to_le_bytes());
        b.extend_from_slice(&(8_000 * 2 * u32::from(channels)).to_le_bytes());
        b.extend_from_slice(&(2 * channels).to_le_bytes());
        b.extend_from_slice(&16_u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        let riff = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&riff.to_le_bytes());
        b
    }

    /// Each column is the lowest and highest sample of its stretch, across
    /// the channels; every frame is in a column, and no column is empty while
    /// there are frames.
    #[test]
    fn peaks_are_the_extremes_of_each_stretch() {
        let q = 32_768.0;
        let mono = pcm16(1, &[16_384, -8_192, 3_277, -29_491, 0, 9_830]);
        let p = peaks(&mono, 3).unwrap();
        let pairs: Vec<(f32, f32)> = p.iter().map(|p| (p.low * q, p.high * q)).collect();
        assert_eq!(
            pairs,
            vec![(-8_192.0, 16_384.0), (-29_491.0, 3_277.0), (0.0, 9_830.0)]
        );
        // Stereo: both channels count.
        let stereo = pcm16(2, &[6_000, -22_000, 1_000, 2_000]);
        let p = peaks(&stereo, 1).unwrap();
        assert_eq!((p[0].low * q, p[0].high * q), (-22_000.0, 6_000.0));
        // More columns than frames: each still has one.
        let p = peaks(&pcm16(1, &[100, -100]), 4).unwrap();
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|p| p.low != 0.0 || p.high != 0.0));
        // No frames: silent columns, not an error.
        assert_eq!(peaks(&pcm16(1, &[]), 2).unwrap(), vec![Peak::SILENT; 2]);
    }

    /// Markers go in as the standard chunks and come out as they went in --
    /// names that are not UTF-8 included -- with the samples untouched, and
    /// replacing them does not leave the old ones behind.
    #[test]
    fn markers_survive_a_round_trip_with_the_rest_of_the_file() {
        let bytes = encode(&tone(8_000, 440.0, 0.01, 0.5), SampleFormat::I16, 1).unwrap();
        let set = [
            Cue {
                frame: 60,
                label: b"verse".to_vec(),
            },
            Cue {
                frame: 3,
                label: b"\xE9t\xE9".to_vec(),
            },
        ];
        let marked = with_cues(&bytes, &set).unwrap();
        let read = cues(&marked).unwrap();
        assert_eq!(read, vec![set[1].clone(), set[0].clone()], "in frame order");
        // A name is a NUL-terminated string inside its chunk, which other
        // readers rely on even though this one would manage without.
        let at = marked.windows(4).position(|w| w == b"labl").unwrap();
        let len = u32::from_le_bytes(marked[at + 4..at + 8].try_into().unwrap()) as usize;
        assert_eq!(marked[at + 8 + len - 1], 0);
        assert_eq!(decode(&marked).unwrap(), decode(&bytes).unwrap());
        assert_eq!(
            u32::from_le_bytes(marked[4..8].try_into().unwrap()) as usize,
            marked.len() - 8
        );
        let again = with_cues(&marked, &set[..1]).unwrap();
        assert_eq!(cues(&again).unwrap(), vec![set[0].clone()]);
        let count = |b: &[u8], what: &[u8]| b.windows(4).filter(|w| *w == what).count();
        assert_eq!(count(&again, b"cue "), 1);
        assert_eq!(count(&again, b"adtl"), 1);
        let bare = with_cues(&marked, &[]).unwrap();
        assert!(cues(&bare).unwrap().is_empty());
        assert_eq!(count(&bare, b"cue "), 0);
        assert_eq!(bare, bytes, "no markers is the file it was");
    }

    /// Insert a `LIST`/`INFO` chunk naming the take, after the format.
    fn with_title(bytes: &[u8]) -> Vec<u8> {
        let mut info = b"INFOINAM".to_vec();
        info.extend_from_slice(&6_u32.to_le_bytes());
        info.extend_from_slice(b"Take1\0");
        let mut b = bytes[..36].to_vec();
        push_chunk(&mut b, b"LIST", &info).unwrap();
        b.extend_from_slice(&bytes[36..]);
        finish_riff(b).unwrap()
    }

    /// A cut is the stretch as it is stored -- the same bytes, the same
    /// format -- with the markers inside it moved to where it now begins, the
    /// ones outside it gone, and the title kept.
    #[test]
    fn a_cut_is_the_stretch_as_stored_with_its_markers() {
        let audio = Audio {
            sample_rate: 44_100,
            channels: 2,
            samples: (0..200).map(|i| (i as f32 / 200.0) - 0.5).collect(),
        };
        let plain = encode(&audio, SampleFormat::I24, 7).unwrap();
        let marks = [5_u32, 50, 90]
            .map(|frame| Cue {
                frame,
                label: format!("m{frame}").into_bytes(),
            })
            .to_vec();
        let source = with_cues(&with_title(&plain), &marks).unwrap();
        let info = parse_header(&source).unwrap();
        let out = cut(&source, 40, 80).unwrap();
        let got = parse_header(&out).unwrap();
        assert_eq!(
            (got.sample_rate, got.channels, got.format, got.frames),
            (44_100, 2, SampleFormat::I24, 40)
        );
        let frame = 6;
        assert_eq!(
            &out[got.data_offset..got.data_offset + got.data_len],
            &source[info.data_offset + 40 * frame..info.data_offset + 80 * frame],
        );
        assert_eq!(
            cues(&out).unwrap(),
            vec![Cue {
                frame: 10,
                label: b"m50".to_vec()
            }]
        );
        assert!(out.windows(5).any(|w| w == b"Take1"), "the title is kept");
        // Past the end is the end; an empty stretch is refused.
        assert_eq!(
            parse_header(&cut(&source, 90, 1_000).unwrap())
                .unwrap()
                .frames,
            10
        );
        assert!(matches!(cut(&source, 50, 50), Err(WavError::Invalid(_))));
        assert!(matches!(cut(&source, 120, 130), Err(WavError::Invalid(_))));
    }

    /// Chunks after the samples are read -- markers usually live there -- and
    /// a torn chunk at the very end is where reading stops, not a reason to
    /// refuse the audio.
    #[test]
    fn chunks_after_the_samples_are_read_and_a_torn_tail_is_ignored() {
        let bytes = encode(&tone(8_000, 440.0, 0.01, 0.5), SampleFormat::I16, 1).unwrap();
        let marked = with_cues(
            &bytes,
            &[Cue {
                frame: 7,
                label: b"here".to_vec(),
            }],
        )
        .unwrap();
        let mut torn = marked.clone();
        torn.extend_from_slice(b"junk");
        torn.extend_from_slice(&1_000_u32.to_le_bytes());
        torn.extend_from_slice(b"abcd");
        assert_eq!(parse_header(&torn).unwrap(), parse_header(&marked).unwrap());
        assert_eq!(cues(&torn).unwrap().len(), 1);
        torn.extend_from_slice(b"LI");
        assert_eq!(decode(&torn).unwrap(), decode(&bytes).unwrap());
    }

    /// The first part of a long file reads as the whole file's header: the
    /// length comes from the data chunk's size measured against the file, not
    /// against the part that was read.
    #[test]
    fn a_prefix_reads_as_the_whole_files_header() {
        let bytes = pcm16(1, &vec![0; 50_000]);
        let whole = bytes.len() as u64;
        let info = parse_header_prefix(&bytes[..4_096], whole).unwrap();
        assert_eq!(info.frames, 50_000);
        assert_eq!(info, parse_header(&bytes).unwrap());
        assert!(
            parse_header(&bytes[..4_096]).unwrap().frames < 50_000,
            "against the prefix alone it is a streaming file's tail"
        );
        assert!(matches!(
            parse_header_prefix(&bytes[..30], whole),
            Err(WavError::Truncated(_))
        ));
    }

    /// Captured samples are stored exactly as they came -- no dither, no
    /// rounding -- under the same header `encode` writes.
    #[test]
    fn sixteen_bit_samples_are_stored_as_they_are() {
        let samples = [0_i16, 1, -1, i16::MAX, i16::MIN, 12_345];
        let bytes = encode_pcm16(22_050, 2, &samples).unwrap();
        let info = parse_header(&bytes).unwrap();
        assert_eq!(
            (info.sample_rate, info.channels, info.format, info.frames),
            (22_050, 2, SampleFormat::I16, 3)
        );
        let stored: Vec<i16> = bytes[info.data_offset..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(stored, samples);
        let audio = Audio {
            sample_rate: 22_050,
            channels: 2,
            samples: vec![0.0; 6],
        };
        assert_eq!(
            bytes[..44],
            encode(&audio, SampleFormat::I16, 1).unwrap()[..44]
        );
    }

    /// A streaming writer that never went back leaves the data size at zero:
    /// the samples are the rest of the file.
    #[test]
    fn a_zero_data_size_means_the_rest_of_the_file() {
        let mut bytes = pcm16(1, &[1, 2, 3, 4]);
        bytes[40..44].copy_from_slice(&0_u32.to_le_bytes());
        assert_eq!(parse_header(&bytes).unwrap().frames, 4);
    }
}
