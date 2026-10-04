//! More than two channels: a multistream packet carries several Opus
//! streams one after another -- each mono or a coupled stereo pair, all but
//! the last self-delimited -- whose channels a mapping table routes to the
//! output (RFC 7845 §5.1.1); and for ambisonics, a demixing matrix that
//! mixes the decoded channels into the output channels (RFC 8486).
//!
//! [`Head`] reads the `OpusHead` an Ogg or Matroska file describes its
//! stream by, and [`Head::decoder`] makes the decoder it calls for.
//!
//! Translated into Rust from libopus 1.5.2's `src/opus_multistream.c`,
//! `src/opus_multistream_decoder.c`, `src/opus_projection_decoder.c` and
//! `src/mapping_matrix.c` (the decoder's parts, `FIXED_POINT`), copyright
//! Xiph.Org, Mozilla, Google and the contributors named in its `COPYING`,
//! used under libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the output buffer the caller sized for `frame_size` samples of each channel (checked on entry), the mapping of one entry a channel, and the matrix of `rows x cols` cells"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "channel and stream counts below 256 and sample counts below 5760; the matrix's products of two 16-bit values fit 32 bits"
)]

use crate::decoder::Decoder;
use crate::error::Error;
use crate::packet::{self, Bandwidth};

/// A multistream decoder (`OpusMSDecoder`): `streams` Opus streams, the
/// first `coupled_streams` of them stereo, routed to `channels` outputs.
pub struct MultistreamDecoder {
    fs: u32,
    channels: usize,
    coupled_streams: usize,
    /// Each output channel's decoded channel: coupled streams' left and
    /// right first (2s, 2s + 1), then the mono streams' (coupled + s); 255
    /// for a silent channel.
    mapping: Vec<u8>,
    decoders: Vec<Decoder>,
}

impl MultistreamDecoder {
    /// `opus_multistream_decoder_create`.
    pub fn new(
        sample_rate: u32,
        channels: usize,
        streams: usize,
        coupled_streams: usize,
        mapping: &[u8],
    ) -> Result<Self, Error> {
        if !(1..=255).contains(&channels)
            || coupled_streams > streams
            || streams < 1
            || streams > 255 - coupled_streams
            || mapping.len() < channels
        {
            return Err(Error::BadArgument);
        }
        let mapping = mapping[..channels].to_vec();
        // `validate_layout`.
        let max_channel = streams + coupled_streams;
        if mapping
            .iter()
            .any(|&m| usize::from(m) >= max_channel && m != 255)
        {
            return Err(Error::BadArgument);
        }
        let decoders = (0..streams)
            .map(|s| Decoder::new(sample_rate, if s < coupled_streams { 2 } else { 1 }))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            fs: sample_rate,
            channels,
            coupled_streams,
            mapping,
            decoders,
        })
    }

    /// The channels decoded.
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// `OPUS_RESET_STATE`, every stream's.
    pub fn reset(&mut self) {
        for d in &mut self.decoders {
            d.reset();
        }
    }

    /// `OPUS_SET_GAIN` for every stream: 1/256 dB.
    pub fn set_gain(&mut self, gain: i32) -> Result<(), Error> {
        for d in &mut self.decoders {
            d.set_gain(gain)?;
        }
        Ok(())
    }

    /// `OPUS_GET_FINAL_RANGE`: the streams' final ranges, XORed (as
    /// libopus's multistream control gives them).
    pub fn final_range(&self) -> u32 {
        self.decoders.iter().fold(0, |r, d| r ^ d.final_range())
    }

    /// `OPUS_GET_LAST_PACKET_DURATION`: the first stream's, as libopus's
    /// multistream control gives it (every stream's packets last as long).
    pub fn last_packet_duration(&self) -> usize {
        self.decoders
            .first()
            .map_or(0, Decoder::last_packet_duration)
    }

    /// `OPUS_SET_PHASE_INVERSION_DISABLED` for every stream.
    pub fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        for d in &mut self.decoders {
            d.set_phase_inversion_disabled(disabled);
        }
    }

    /// `OPUS_GET_BANDWIDTH`: the first stream's last packet's.
    pub fn bandwidth(&self) -> Option<Bandwidth> {
        self.decoders.first().and_then(Decoder::bandwidth)
    }

    /// `opus_multistream_decode`: as [`Decoder::decode`], into `pcm`
    /// interleaved over [`Self::channels`].
    pub fn decode(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [i16],
        fec: bool,
    ) -> Result<usize, Error> {
        let channels = self.channels;
        self.decode_native(packet, pcm.len() / channels, fec, |chan, src, stride, n| {
            for i in 0..n {
                pcm[i * channels + chan] = src.map_or(0, |s| s[i * stride]);
            }
        })
    }

    /// `opus_multistream_decode_float`.
    pub fn decode_float(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        fec: bool,
    ) -> Result<usize, Error> {
        let channels = self.channels;
        self.decode_native(packet, pcm.len() / channels, fec, |chan, src, stride, n| {
            for i in 0..n {
                pcm[i * channels + chan] =
                    src.map_or(0.0, |s| (1.0 / 32768.0) * f32::from(s[i * stride]));
            }
        })
    }

    /// `opus_multistream_decode_native`: each stream decoded, its channels
    /// handed to `copy_out(output channel, samples or None for silence,
    /// stride, count)`.
    pub(crate) fn decode_native(
        &mut self,
        data: Option<&[u8]>,
        frame_size: usize,
        fec: bool,
        mut copy_out: impl FnMut(usize, Option<&[i16]>, usize, usize),
    ) -> Result<usize, Error> {
        if frame_size == 0 {
            return Err(Error::BadArgument);
        }
        // No more than 120 ms.
        let mut frame_size = frame_size.min(self.fs as usize / 25 * 3);
        let mut buf = vec![0i16; 2 * frame_size];
        let streams = self.decoders.len();
        let data = data.unwrap_or(&[]);
        let do_plc = data.is_empty();
        if !do_plc && data.len() < 2 * streams - 1 {
            return Err(Error::InvalidPacket);
        }
        if !do_plc {
            let samples = validate(data, streams, self.fs)?;
            if samples > frame_size {
                return Err(Error::BufferTooSmall);
            }
        }
        let mut rest = data;
        for s in 0..streams {
            if !do_plc && rest.is_empty() {
                return Err(Error::Internal);
            }
            let self_delimited = s != streams - 1;
            let ch = if s < self.coupled_streams { 2 } else { 1 };
            let packet = if do_plc { None } else { Some(rest) };
            let (n, offset) = self.decoders[s].decode_native(
                packet,
                &mut buf[..frame_size * ch],
                frame_size,
                fec,
                self_delimited,
            )?;
            if !do_plc {
                rest = rest.get(offset..).unwrap_or(&[]);
            }
            if n == 0 {
                return Ok(0);
            }
            frame_size = n;
            if s < self.coupled_streams {
                // The left to wherever it goes, then the right.
                for (chan, &m) in self.mapping.iter().enumerate() {
                    if usize::from(m) == 2 * s {
                        copy_out(chan, Some(&buf[..]), 2, frame_size);
                    }
                }
                for (chan, &m) in self.mapping.iter().enumerate() {
                    if usize::from(m) == 2 * s + 1 {
                        copy_out(chan, Some(&buf[1..]), 2, frame_size);
                    }
                }
            } else {
                for (chan, &m) in self.mapping.iter().enumerate() {
                    if usize::from(m) == s + self.coupled_streams {
                        copy_out(chan, Some(&buf[..]), 1, frame_size);
                    }
                }
            }
        }
        // Silent channels.
        for (chan, &m) in self.mapping.iter().enumerate() {
            if m == 255 {
                copy_out(chan, None, 0, frame_size);
            }
        }
        Ok(frame_size)
    }
}

/// `opus_multistream_packet_validate`: every stream's packet sound and of
/// one duration; that duration.
fn validate(data: &[u8], streams: usize, fs: u32) -> Result<usize, Error> {
    let mut rest = data;
    let mut samples = 0;
    for s in 0..streams {
        if rest.is_empty() {
            return Err(Error::InvalidPacket);
        }
        let parsed = packet::parse(rest, s != streams - 1)?;
        let this = packet::packet_samples(&rest[..parsed.packet_offset.min(rest.len())], fs)?;
        if s != 0 && samples != this {
            return Err(Error::InvalidPacket);
        }
        samples = this;
        rest = rest.get(parsed.packet_offset..).unwrap_or(&[]);
    }
    Ok(samples)
}

/// An ambisonics decoder (`OpusProjectionDecoder`): a multistream decoder
/// whose decoded channels a demixing matrix mixes into the output.
pub struct ProjectionDecoder {
    inner: MultistreamDecoder,
    /// The matrix, column by column: `rows` (output channels) a column,
    /// one column a decoded channel; Q15.
    matrix: Vec<i16>,
    rows: usize,
}

impl ProjectionDecoder {
    /// `opus_projection_decoder_create`: `demixing_matrix` as RFC 8486
    /// stores it, little-endian 16-bit cells, `(streams + coupled) x
    /// channels` of them.
    pub fn new(
        sample_rate: u32,
        channels: usize,
        streams: usize,
        coupled_streams: usize,
        demixing_matrix: &[u8],
    ) -> Result<Self, Error> {
        let inputs = streams + coupled_streams;
        if demixing_matrix.len() != inputs * channels * 2 || inputs > 255 || channels > 255 {
            return Err(Error::BadArgument);
        }
        if inputs * channels * 2 > 65004 {
            return Err(Error::BadArgument);
        }
        let matrix = demixing_matrix
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        // Each decoded channel to the matrix column of its own number.
        let mapping: Vec<u8> = (0..channels).map(|i| i as u8).collect();
        let inner =
            MultistreamDecoder::new(sample_rate, channels, streams, coupled_streams, &mapping)?;
        Ok(Self {
            inner,
            matrix,
            rows: channels,
        })
    }

    /// The channels decoded.
    pub const fn channels(&self) -> usize {
        self.rows
    }

    /// `OPUS_RESET_STATE`.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// `OPUS_SET_GAIN` for every stream: 1/256 dB, applied to each decoded
    /// channel before the matrix.
    pub fn set_gain(&mut self, gain: i32) -> Result<(), Error> {
        self.inner.set_gain(gain)
    }

    /// `OPUS_GET_FINAL_RANGE`: the streams' final ranges, XORed.
    pub fn final_range(&self) -> u32 {
        self.inner.final_range()
    }

    /// `OPUS_GET_LAST_PACKET_DURATION`: the first stream's.
    pub fn last_packet_duration(&self) -> usize {
        self.inner.last_packet_duration()
    }

    /// `OPUS_SET_PHASE_INVERSION_DISABLED` for every stream.
    pub fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        self.inner.set_phase_inversion_disabled(disabled);
    }

    /// `OPUS_GET_BANDWIDTH`: the first stream's last packet's.
    pub fn bandwidth(&self) -> Option<Bandwidth> {
        self.inner.bandwidth()
    }

    /// `opus_projection_decode`: each decoded channel times its column of
    /// the matrix, summed into the output.
    pub fn decode(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [i16],
        fec: bool,
    ) -> Result<usize, Error> {
        let rows = self.rows;
        let matrix = &self.matrix;
        self.inner
            .decode_native(packet, pcm.len() / rows, fec, |chan, src, stride, n| {
                // Cleared when the first channel arrives.
                if chan == 0 {
                    pcm[..n * rows].fill(0);
                }
                if let Some(src) = src {
                    // `mapping_matrix_multiply_channel_out_short`: the sum
                    // wraps to 16 bits, as C's `+=` into a short does.
                    for i in 0..n {
                        let sample = i32::from(src[i * stride]);
                        for row in 0..rows {
                            let tmp = i32::from(matrix[rows * chan + row]) * sample;
                            let at = rows * i + row;
                            pcm[at] = (i32::from(pcm[at]) + ((tmp + 16384) >> 15)) as i16;
                        }
                    }
                }
            })
    }

    /// `opus_projection_decode_float`: as [`Self::decode`], the matrix
    /// applied in single-precision floats -- each product and sum rounded
    /// in libopus's order, so the result is libopus's to the bit.
    pub fn decode_float(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        fec: bool,
    ) -> Result<usize, Error> {
        let rows = self.rows;
        let matrix = &self.matrix;
        self.inner
            .decode_native(packet, pcm.len() / rows, fec, |chan, src, stride, n| {
                if chan == 0 {
                    pcm[..n * rows].fill(0.0);
                }
                if let Some(src) = src {
                    // `mapping_matrix_multiply_channel_out_float`.
                    for i in 0..n {
                        let sample = (1.0 / 32768.0) * f32::from(src[i * stride]);
                        for row in 0..rows {
                            let tmp =
                                (1.0 / 32768.0) * f32::from(matrix[rows * chan + row]) * sample;
                            pcm[rows * i + row] += tmp;
                        }
                    }
                }
            })
    }
}

/// An Opus stream's description: `OpusHead` (RFC 7845 §5.1), as an Ogg
/// stream's first packet and a Matroska track's `CodecPrivate` hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// Output channels.
    pub channels: usize,
    /// Samples (at 48 kHz) to drop from the start of the decoded stream.
    pub pre_skip: u16,
    /// The encoder's input rate -- for information only: Opus decodes at
    /// 48 kHz (or any of its rates) whatever it was.
    pub input_sample_rate: u32,
    /// A gain to apply, Q7.8 dB.
    pub output_gain: i16,
    /// How the channels are coded: 0 (mono or stereo), 1 (Vorbis's
    /// surround orders), 2 or 3 (ambisonics), 255 (no defined layout).
    pub mapping_family: u8,
    pub streams: usize,
    pub coupled_streams: usize,
    /// Each output channel's decoded channel (families other than 3).
    pub mapping: Vec<u8>,
    /// The demixing matrix (family 3).
    pub demixing_matrix: Vec<u8>,
}

/// A decoder for a [`Head`]: one stream, several, or ambisonics.
pub enum AnyDecoder {
    Single(Box<Decoder>),
    Multistream(Box<MultistreamDecoder>),
    Projection(Box<ProjectionDecoder>),
}

impl AnyDecoder {
    /// The channels decoded.
    pub fn channels(&self) -> usize {
        match self {
            Self::Single(d) => d.channels(),
            Self::Multistream(d) => d.channels(),
            Self::Projection(d) => d.channels(),
        }
    }

    /// As [`Decoder::decode`].
    pub fn decode(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [i16],
        fec: bool,
    ) -> Result<usize, Error> {
        match self {
            Self::Single(d) => d.decode(packet, pcm, fec),
            Self::Multistream(d) => d.decode(packet, pcm, fec),
            Self::Projection(d) => d.decode(packet, pcm, fec),
        }
    }

    /// As [`Decoder::decode_float`].
    pub fn decode_float(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        fec: bool,
    ) -> Result<usize, Error> {
        match self {
            Self::Single(d) => d.decode_float(packet, pcm, fec),
            Self::Multistream(d) => d.decode_float(packet, pcm, fec),
            Self::Projection(d) => d.decode_float(packet, pcm, fec),
        }
    }

    /// `OPUS_RESET_STATE`.
    pub fn reset(&mut self) {
        match self {
            Self::Single(d) => d.reset(),
            Self::Multistream(d) => d.reset(),
            Self::Projection(d) => d.reset(),
        }
    }

    /// `OPUS_SET_GAIN`: 1/256 dB, -32768 to 32767. [`Head::decoder`] has
    /// already set the head's.
    pub fn set_gain(&mut self, gain: i32) -> Result<(), Error> {
        match self {
            Self::Single(d) => d.set_gain(gain),
            Self::Multistream(d) => d.set_gain(gain),
            Self::Projection(d) => d.set_gain(gain),
        }
    }

    /// `OPUS_SET_PHASE_INVERSION_DISABLED`.
    pub fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        match self {
            Self::Single(d) => d.set_phase_inversion_disabled(disabled),
            Self::Multistream(d) => d.set_phase_inversion_disabled(disabled),
            Self::Projection(d) => d.set_phase_inversion_disabled(disabled),
        }
    }

    /// `OPUS_GET_FINAL_RANGE`: of a multistream decoder, the streams'
    /// XORed.
    pub fn final_range(&self) -> u32 {
        match self {
            Self::Single(d) => d.final_range(),
            Self::Multistream(d) => d.final_range(),
            Self::Projection(d) => d.final_range(),
        }
    }

    /// `OPUS_GET_LAST_PACKET_DURATION`: the samples a channel the last
    /// packet decoded (or concealed) held.
    pub fn last_packet_duration(&self) -> usize {
        match self {
            Self::Single(d) => d.last_packet_duration(),
            Self::Multistream(d) => d.last_packet_duration(),
            Self::Projection(d) => d.last_packet_duration(),
        }
    }

    /// `OPUS_GET_BANDWIDTH`: the last packet's (of several streams, the
    /// first's); `None` before any.
    pub fn bandwidth(&self) -> Option<Bandwidth> {
        match self {
            Self::Single(d) => d.bandwidth(),
            Self::Multistream(d) => d.bandwidth(),
            Self::Projection(d) => d.bandwidth(),
        }
    }
}

impl Head {
    /// `OpusHead` parsed: `None` for one that is not, or of a version this
    /// cannot read (a major version other than 0), or inconsistent.
    pub fn parse(head: &[u8]) -> Option<Self> {
        let rest = head.strip_prefix(b"OpusHead")?;
        let (&version, rest) = rest.split_first()?;
        // Versions 0 to 15 are compatible; the major version is the high
        // nibble.
        if version >> 4 != 0 {
            return None;
        }
        let (&channels, rest) = rest.split_first()?;
        let pre_skip = u16::from_le_bytes(rest.get(..2)?.try_into().ok()?);
        let input_sample_rate = u32::from_le_bytes(rest.get(2..6)?.try_into().ok()?);
        let output_gain = i16::from_le_bytes(rest.get(6..8)?.try_into().ok()?);
        let &mapping_family = rest.get(8)?;
        let channels = usize::from(channels);
        if channels == 0 {
            return None;
        }
        let rest = &rest[9..];
        let (streams, coupled_streams, mapping, demixing_matrix) = if mapping_family == 0 {
            if channels > 2 {
                return None;
            }
            (1, channels - 1, (0..channels as u8).collect(), Vec::new())
        } else {
            let streams = usize::from(*rest.first()?);
            let coupled = usize::from(*rest.get(1)?);
            if streams == 0 || coupled > streams || streams + coupled > 255 {
                return None;
            }
            if mapping_family == 3 {
                let size = (streams + coupled) * channels * 2;
                (
                    streams,
                    coupled,
                    Vec::new(),
                    rest.get(2..2 + size)?.to_vec(),
                )
            } else {
                (
                    streams,
                    coupled,
                    rest.get(2..2 + channels)?.to_vec(),
                    Vec::new(),
                )
            }
        };
        Some(Self {
            channels,
            pre_skip,
            input_sample_rate,
            output_gain,
            mapping_family,
            streams,
            coupled_streams,
            mapping,
            demixing_matrix,
        })
    }

    /// The decoder this stream needs, at `sample_rate`, its output gain
    /// set.
    pub fn decoder(&self, sample_rate: u32) -> Result<AnyDecoder, Error> {
        let gain = i32::from(self.output_gain);
        if self.mapping_family == 3 {
            let mut d = ProjectionDecoder::new(
                sample_rate,
                self.channels,
                self.streams,
                self.coupled_streams,
                &self.demixing_matrix,
            )?;
            d.set_gain(gain)?;
            return Ok(AnyDecoder::Projection(Box::new(d)));
        }
        // One stream whose channels are the output's, in order, is a plain
        // decoder's work -- but only if it is stereo exactly when the output
        // is: a coupled stream to one channel is its left alone (libopus's
        // multistream decoder), which a mono decoder would mix down instead.
        if self.streams == 1
            && self.coupled_streams + 1 == self.channels
            && self
                .mapping
                .iter()
                .enumerate()
                .all(|(i, &m)| usize::from(m) == i)
        {
            let mut d = Decoder::new(sample_rate, self.channels)?;
            d.set_gain(gain)?;
            return Ok(AnyDecoder::Single(Box::new(d)));
        }
        let mut d = MultistreamDecoder::new(
            sample_rate,
            self.channels,
            self.streams,
            self.coupled_streams,
            &self.mapping,
        )?;
        d.set_gain(gain)?;
        Ok(AnyDecoder::Multistream(Box::new(d)))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    fn head(family: u8, channels: u8, table: &[u8]) -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.push(1);
        h.push(channels);
        h.extend(312u16.to_le_bytes());
        h.extend(48000u32.to_le_bytes());
        h.extend((-256i16).to_le_bytes());
        h.push(family);
        h.extend(table);
        h
    }

    #[test]
    fn heads_parse_by_family() {
        let h = Head::parse(&head(0, 2, &[])).expect("stereo");
        assert_eq!(
            (
                h.channels,
                h.streams,
                h.coupled_streams,
                h.pre_skip,
                h.output_gain
            ),
            (2, 1, 1, 312, -256)
        );
        assert_eq!(h.mapping, [0, 1]);
        // 5.1: four streams, two coupled; FL FC FR RL RR LFE.
        let h = Head::parse(&head(1, 6, &[4, 2, 0, 4, 1, 2, 3, 5])).expect("5.1");
        assert_eq!((h.streams, h.coupled_streams), (4, 2));
        assert_eq!(h.mapping, [0, 4, 1, 2, 3, 5]);
        assert!(h.decoder(48000).is_ok());
        // Not OpusHead; version 1.x; more coupled streams than streams.
        assert!(Head::parse(b"OpusTags").is_none());
        let mut v16 = head(0, 1, &[]);
        v16[8] = 16;
        assert!(Head::parse(&v16).is_none());
        assert!(Head::parse(&head(1, 2, &[1, 2, 0, 1])).is_none());
    }

    #[test]
    fn a_coupled_stream_to_one_channel_is_its_left() {
        // One stream, coupled, to one output channel: libopus's multistream
        // decoder decodes it in stereo and keeps the left. A mono decoder
        // would mix the two down instead.
        let h = Head::parse(&head(1, 1, &[1, 1, 0])).expect("a head");
        let mut ours = h.decoder(48000).expect("a decoder");
        assert!(matches!(ours, AnyDecoder::Multistream(_)));
        let mut stereo = Decoder::new(48000, 2).expect("a decoder");
        stereo
            .set_gain(i32::from(h.output_gain))
            .expect("the head's gain");
        // A CELT fullband stereo frame: any payload decodes to something.
        let mut seed = 7u32;
        for _ in 0..8 {
            let mut packet = vec![(31 << 3) | 0x4];
            packet.extend((0..120).map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as u8
            }));
            let mut one = vec![0i16; 960];
            let mut two = vec![0i16; 1920];
            assert_eq!(ours.decode(Some(&packet), &mut one, false), Ok(960));
            assert_eq!(stereo.decode(Some(&packet), &mut two, false), Ok(960));
            let left: Vec<i16> = two.iter().step_by(2).copied().collect();
            assert_eq!(one, left);
            assert!(left.iter().any(|&s| s != 0), "a silent frame tests nothing");
        }
        // Stereo to two channels in order is a plain decoder's work.
        let h = Head::parse(&head(1, 2, &[1, 1, 0, 1])).expect("a head");
        assert!(matches!(h.decoder(48000), Ok(AnyDecoder::Single(_))));
    }

    #[test]
    fn a_mapping_past_the_streams_is_refused() {
        assert!(MultistreamDecoder::new(48000, 2, 1, 0, &[0, 1]).is_err());
        assert!(MultistreamDecoder::new(48000, 2, 1, 0, &[0, 255]).is_ok());
    }
}
