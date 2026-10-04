//! The Opus decoder (RFC 6716 §4): a packet's frames, each decoded by SILK,
//! CELT or both (hybrid), with the transitions between modes smoothed --
//! CELT's redundant 5 ms frames, cross-fades from concealed audio -- and the
//! concealment of lost packets, or their recovery from the next packet's
//! low-bitrate redundancy (FEC).
//!
//! Translated into Rust from libopus 1.5.2's `src/opus_decoder.c`, built
//! `FIXED_POINT` (16-bit output, CELT adding onto SILK's samples) without
//! deep PLC, DRED or OSCE, copyright Xiph.Org, Skype and the contributors
//! named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the output buffer the caller sized for `frame_size` samples a channel (checked on entry) or the frame's own bytes, at the offsets libopus computes"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample counts below 5760 a channel and byte counts below a packet's; the sample arithmetic saturates or wraps through the fixed-point helpers"
)]

use crate::celt::decoder::CeltDecoder;
use crate::celt::mathops::exp2;
use crate::celt::mode::window;
use crate::entdec::Decoder as RangeDecoder;
use crate::error::Error;
use crate::fixed::{
    Q15ONE, mac16_16, mult16_16, mult16_16_p15, mult16_16_q15, mult16_32_p16, qconst16, saturate,
    saturate16,
};
use crate::packet::{self, Bandwidth, Mode};
use crate::silk::{DecControl, LostFlag, SilkDecoder};

/// `smooth_fade`: `overlap` samples a channel cross-faded from `in1` to
/// `in2` by the CELT window squared (subsampled to `fs`), into `out`.
fn smooth_fade(
    in1: &[i16],
    in2: &[i16],
    out: &mut [i16],
    overlap: usize,
    channels: usize,
    fs: usize,
) {
    let inc = 48000 / fs;
    let window = window();
    for c in 0..channels {
        for i in 0..overlap {
            let w = i32::from(window[i * inc]);
            let w = mult16_16_q15(w, w);
            let k = i * channels + c;
            out[k] = (mac16_16(
                mult16_16(w, i32::from(in2[k])),
                Q15ONE - w,
                i32::from(in1[k]),
            ) >> 15) as i16;
        }
    }
}

/// An Opus decoder: one stream of one or two channels, decoded at 8, 12,
/// 16, 24 or 48 kHz.
pub struct Decoder {
    channels: usize,
    /// The output rate.
    fs: usize,
    silk: SilkDecoder,
    celt: CeltDecoder,
    dec_control: DecControl,
    /// The output gain, in 1/256 dB (`OPUS_SET_GAIN`).
    decode_gain: i32,

    // What a reset clears.
    /// The channels the last packet coded.
    stream_channels: usize,
    bandwidth: Option<Bandwidth>,
    mode: Option<Mode>,
    prev_mode: Option<Mode>,
    /// Samples a frame of the last packet held, a channel.
    frame_size: usize,
    /// The last frame ended with a redundant CELT frame (SILK to CELT).
    prev_redundancy: bool,
    last_packet_duration: usize,
    range_final: u32,
}

impl Decoder {
    /// `opus_decoder_create`: a decoder of `channels` (1 or 2) at
    /// `sample_rate` (8000, 12000, 16000, 24000 or 48000).
    pub fn new(sample_rate: u32, channels: usize) -> Result<Self, Error> {
        if !matches!(sample_rate, 48000 | 24000 | 16000 | 12000 | 8000)
            || !(1..=2).contains(&channels)
        {
            return Err(Error::BadArgument);
        }
        let fs = sample_rate as usize;
        // (`CELT_SET_SIGNALLING(0)` is the standard mode's only setting.)
        let celt = CeltDecoder::new(sample_rate, channels)?;
        Ok(Self {
            channels,
            fs,
            silk: SilkDecoder::new(),
            celt,
            dec_control: DecControl {
                channels_api: channels,
                channels_internal: channels,
                api_sample_rate: sample_rate as i32,
                internal_sample_rate: 0,
                payload_size_ms: 0,
                prev_pitch_lag: 0,
            },
            decode_gain: 0,
            stream_channels: channels,
            bandwidth: None,
            mode: None,
            prev_mode: None,
            frame_size: fs / 400,
            prev_redundancy: false,
            last_packet_duration: 0,
            range_final: 0,
        })
    }

    /// The channels decoded.
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// The output rate.
    pub const fn sample_rate(&self) -> u32 {
        self.fs as u32
    }

    /// `OPUS_RESET_STATE`: the decoder as new, for a fresh stream.
    pub fn reset(&mut self) {
        self.stream_channels = self.channels;
        self.bandwidth = None;
        self.mode = None;
        self.prev_mode = None;
        self.frame_size = self.fs / 400;
        self.prev_redundancy = false;
        self.last_packet_duration = 0;
        self.range_final = 0;
        self.celt.reset();
        self.silk.reset();
    }

    /// `OPUS_GET_FINAL_RANGE`: the range decoder's final state for the last
    /// packet -- the encoder's for an intact one, which is how a test checks
    /// a decoder against an encoder.
    pub const fn final_range(&self) -> u32 {
        self.range_final
    }

    /// `OPUS_GET_LAST_PACKET_DURATION`: samples a channel the last call
    /// produced.
    pub const fn last_packet_duration(&self) -> usize {
        self.last_packet_duration
    }

    /// `OPUS_GET_BANDWIDTH`: the last packet's bandwidth.
    pub const fn bandwidth(&self) -> Option<Bandwidth> {
        self.bandwidth
    }

    /// `OPUS_GET_PITCH`: the last frame's pitch period, in samples at 48 kHz
    /// (0 for an unvoiced SILK frame).
    pub fn pitch(&self) -> i32 {
        if self.prev_mode == Some(Mode::Celt) {
            self.celt.pitch()
        } else {
            self.dec_control.prev_pitch_lag
        }
    }

    /// `OPUS_SET_GAIN`: a gain applied to the output, in 1/256 dB (-128 to
    /// +128 dB).
    pub fn set_gain(&mut self, gain: i32) -> Result<(), Error> {
        if !(-32768..=32767).contains(&gain) {
            return Err(Error::BadArgument);
        }
        self.decode_gain = gain;
        Ok(())
    }

    /// `OPUS_GET_GAIN`.
    pub const fn gain(&self) -> i32 {
        self.decode_gain
    }

    /// `OPUS_SET_PHASE_INVERSION_DISABLED`: never invert a stereo band's
    /// phase, for output that will be mixed down to mono.
    pub fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        self.celt.set_phase_inversion_disabled(disabled);
    }

    /// `opus_decode`: one packet into `pcm` (interleaved, room for
    /// `pcm.len() / channels` samples a channel); the samples a channel
    /// written. `None` (or an empty packet) conceals a lost packet, filling
    /// all of `pcm`, whose length must then be a multiple of 2.5 ms. With
    /// `fec`, `packet` is the one after a lost packet, and the lost one's
    /// audio is recovered from its redundancy where it has some (else
    /// concealed): `pcm` must then be the lost packet's length.
    pub fn decode(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [i16],
        fec: bool,
    ) -> Result<usize, Error> {
        let frame_size = pcm.len() / self.channels;
        if frame_size == 0 {
            return Err(Error::BadArgument);
        }
        self.decode_native(packet, pcm, frame_size, fec, false)
            .map(|(n, _)| n)
    }

    /// `opus_decode_float`: as [`Self::decode`], the samples as floats in
    /// [-1, 1).
    pub fn decode_float(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        fec: bool,
    ) -> Result<usize, Error> {
        let mut frame_size = pcm.len() / self.channels;
        if frame_size == 0 {
            return Err(Error::BadArgument);
        }
        if let Some(p) = packet.filter(|p| !p.is_empty() && !fec) {
            let n = packet::packet_samples(p, self.fs as u32)?;
            if n == 0 {
                return Err(Error::InvalidPacket);
            }
            frame_size = frame_size.min(n);
        }
        let mut out = vec![0i16; frame_size * self.channels];
        let (n, _) = self.decode_native(packet, &mut out, frame_size, fec, false)?;
        for (f, &s) in pcm.iter_mut().zip(&out[..n * self.channels]) {
            *f = f32::from(s) * (1.0 / 32768.0);
        }
        Ok(n)
    }

    /// `opus_decode_native`: the samples a channel decoded and the bytes the
    /// packet took (with its padding: where a self-delimited packet's
    /// successor begins).
    pub(crate) fn decode_native(
        &mut self,
        data: Option<&[u8]>,
        pcm: &mut [i16],
        frame_size: usize,
        decode_fec: bool,
        self_delimited: bool,
    ) -> Result<(usize, usize), Error> {
        let ch = self.channels;
        if pcm.len() < frame_size * ch {
            return Err(Error::BufferTooSmall);
        }
        let data = data.filter(|d| !d.is_empty());
        // FEC and concealment need a whole number of 2.5 ms.
        if (decode_fec || data.is_none()) && !frame_size.is_multiple_of(self.fs / 400) {
            return Err(Error::BadArgument);
        }
        let Some(data) = data else {
            let mut count = 0;
            while count < frame_size {
                count +=
                    self.decode_frame(None, 0, &mut pcm[count * ch..], frame_size - count, false)?;
            }
            self.last_packet_duration = count;
            return Ok((count, 0));
        };
        let toc = data[0];
        let packet_mode = packet::mode(toc);
        let packet_bandwidth = packet::bandwidth(toc);
        let packet_frame_size = packet::samples_per_frame(toc, self.fs);
        let packet_stream_channels = packet::channels(toc);
        let parsed = packet::parse(data, self_delimited)?;
        let frames = &data[parsed.payload_offset..];

        if decode_fec {
            // No FEC possible: conceal -- the packet still having been
            // parsed, so that a multistream packet's next stream is found
            // after this one's.
            if frame_size < packet_frame_size
                || packet_mode == Mode::Celt
                || self.mode == Some(Mode::Celt)
            {
                return self
                    .decode_native(None, pcm, frame_size, false, false)
                    .map(|(n, _)| (n, parsed.packet_offset));
            }
            // Conceal all but the part FEC may cover.
            let duration_copy = self.last_packet_duration;
            if frame_size > packet_frame_size {
                if let Err(e) =
                    self.decode_native(None, pcm, frame_size - packet_frame_size, false, false)
                {
                    self.last_packet_duration = duration_copy;
                    return Err(e);
                }
            }
            // The rest from the FEC.
            self.mode = Some(packet_mode);
            self.bandwidth = Some(packet_bandwidth);
            self.frame_size = packet_frame_size;
            self.stream_channels = packet_stream_channels;
            let size = parsed.sizes[0];
            self.decode_frame(
                Some(&frames[..size]),
                size,
                &mut pcm[ch * (frame_size - packet_frame_size)..],
                packet_frame_size,
                true,
            )?;
            self.last_packet_duration = frame_size;
            return Ok((frame_size, parsed.packet_offset));
        }

        if parsed.count * packet_frame_size > frame_size {
            return Err(Error::BufferTooSmall);
        }
        // The state changed only now, so that an invalid packet leaves it.
        self.mode = Some(packet_mode);
        self.bandwidth = Some(packet_bandwidth);
        self.frame_size = packet_frame_size;
        self.stream_channels = packet_stream_channels;

        let mut nb_samples = 0;
        let mut at = 0;
        for &size in &parsed.sizes[..parsed.count] {
            let frame = &frames[at..at + size];
            nb_samples += self.decode_frame(
                Some(frame),
                size,
                &mut pcm[nb_samples * ch..],
                frame_size - nb_samples,
                false,
            )?;
            at += size;
        }
        self.last_packet_duration = nb_samples;
        Ok((nb_samples, parsed.packet_offset))
    }

    /// `opus_decode_frame`: one frame of `len` bytes (`data`, or `None` to
    /// conceal one) into `pcm`, which has room for `frame_size` samples a
    /// channel; the samples a channel written.
    fn decode_frame(
        &mut self,
        data: Option<&[u8]>,
        len: usize,
        pcm: &mut [i16],
        frame_size: usize,
        decode_fec: bool,
    ) -> Result<usize, Error> {
        let ch = self.channels;
        let fs = self.fs;
        let f20 = fs / 50;
        let f10 = f20 >> 1;
        let f5 = f10 >> 1;
        let f2_5 = f5 >> 1;
        if frame_size < f2_5 {
            return Err(Error::BufferTooSmall);
        }
        // No more than 120 ms.
        let mut frame_size = frame_size.min(fs / 25 * 3);
        // A payload of a byte or none (two or one with the TOC): concealment
        // or DTX, of no more than the TOC says.
        let mut len = len as isize;
        let data = if len <= 1 {
            frame_size = frame_size.min(self.frame_size);
            None
        } else {
            data
        };

        let mut audiosize;
        let mode;
        let bandwidth;
        let mut dec = RangeDecoder::new(data.unwrap_or(&[]));
        if data.is_some() {
            audiosize = self.frame_size;
            mode = self.mode;
            bandwidth = self.bandwidth;
        } else {
            audiosize = frame_size;
            // Conceal in the last mode (CELT after CELT redundancy).
            mode = if self.prev_redundancy {
                Some(Mode::Celt)
            } else {
                self.prev_mode
            };
            bandwidth = None;
            if mode.is_none() {
                // Nothing decoded yet: silence.
                pcm[..audiosize * ch].fill(0);
                return Ok(audiosize);
            }
            // Conceal only in 2.5, 5, 10 or 20 ms pieces (not 12.5 or 30).
            if audiosize > f20 {
                let mut at = 0;
                while audiosize > 0 {
                    let ret =
                        self.decode_frame(None, 0, &mut pcm[at * ch..], audiosize.min(f20), false)?;
                    at += ret;
                    audiosize = audiosize.saturating_sub(ret);
                }
                return Ok(frame_size);
            } else if audiosize < f20 {
                if audiosize > f10 {
                    audiosize = f10;
                } else if mode != Some(Mode::Silk) && audiosize > f5 && audiosize < f10 {
                    audiosize = f5;
                }
            }
        }
        let Some(mode) = mode else {
            return Err(Error::Internal);
        };

        // In fixed point CELT adds onto SILK's samples in `pcm` -- when
        // `pcm` has room for the 10 ms SILK always produces.
        let celt_accum = mode != Mode::Celt && frame_size >= f10;

        let mut transition = data.is_some()
            && self.prev_mode.is_some()
            && ((mode == Mode::Celt
                && self.prev_mode != Some(Mode::Celt)
                && !self.prev_redundancy)
                || (mode != Mode::Celt && self.prev_mode == Some(Mode::Celt)));
        let mut pcm_transition: Vec<i16> = Vec::new();
        if transition && mode == Mode::Celt {
            pcm_transition = vec![0; f5 * ch];
            // The concealed audio to fade from: its failure leaves silence
            // to fade from, as libopus's ignored return does.
            let _ = self.decode_frame(None, 0, &mut pcm_transition, f5.min(audiosize), false);
        }
        if audiosize > frame_size {
            return Err(Error::BadArgument);
        }
        frame_size = audiosize;

        let mut pcm_silk: Vec<i16> = if mode != Mode::Celt && !celt_accum {
            vec![0; f10.max(frame_size) * ch]
        } else {
            Vec::new()
        };

        // SILK.
        if mode != Mode::Celt {
            if self.prev_mode == Some(Mode::Celt) {
                self.silk.reset();
            }
            // SILK's concealment makes no less than 10 ms.
            self.dec_control.payload_size_ms = 10.max(1000 * audiosize / fs) as i32;
            if data.is_some() {
                self.dec_control.channels_internal = self.stream_channels;
                self.dec_control.internal_sample_rate = if mode == Mode::Silk {
                    match bandwidth {
                        Some(Bandwidth::Narrowband) => 8000,
                        Some(Bandwidth::Mediumband) => 12000,
                        _ => 16000,
                    }
                } else {
                    16000
                };
            }
            let lost_flag = if data.is_none() {
                LostFlag::Lost
            } else if decode_fec {
                LostFlag::Lbrr
            } else {
                LostFlag::Normal
            };
            let out: &mut [i16] = if celt_accum { &mut *pcm } else { &mut pcm_silk };
            let mut decoded = 0;
            loop {
                let first_frame = decoded == 0;
                let at = (decoded * ch).min(out.len());
                let silk_frame_size = match self.silk.decode(
                    &mut self.dec_control,
                    lost_flag,
                    first_frame,
                    &mut dec,
                    &mut out[at..],
                ) {
                    Ok(n) => n,
                    Err(()) if lost_flag == LostFlag::Lost => {
                        // A concealment failure is not fatal: silence.
                        let end = (at + frame_size * ch).min(out.len());
                        out[at..end].fill(0);
                        frame_size
                    }
                    Err(()) => return Err(Error::Internal),
                };
                decoded += silk_frame_size;
                if decoded >= frame_size {
                    break;
                }
            }
        }

        let mut redundancy = false;
        let mut celt_to_silk = false;
        let mut redundancy_bytes: isize = 0;
        if !decode_fec
            && mode != Mode::Celt
            && data.is_some()
            && dec.tell() as isize + 17 + 20 * isize::from(mode == Mode::Hybrid) <= 8 * len
        {
            // A redundant 0-8 kHz CELT frame.
            redundancy = mode != Mode::Hybrid || dec.bit_logp(12);
            if redundancy {
                celt_to_silk = dec.bit_logp(1);
                // At least two bytes, in the SILK case for the check above.
                redundancy_bytes = if mode == Mode::Hybrid {
                    dec.uint(256) as isize + 2
                } else {
                    len - ((dec.tell() as isize + 7) >> 3)
                };
                len -= redundancy_bytes;
                // Not for a valid packet; the behaviour is not normative.
                if len * 8 < dec.tell() as isize {
                    len = 0;
                    redundancy_bytes = 0;
                    redundancy = false;
                }
                // The raw bits are the redundant frame's, not this frame's.
                dec.storage = dec.storage.wrapping_sub(redundancy_bytes as u32);
            }
        }
        let start_band = if mode == Mode::Celt { 0 } else { 17 };
        if redundancy {
            transition = false;
        }
        if transition && mode != Mode::Celt {
            pcm_transition = vec![0; f5 * ch];
            // As above.
            let _ = self.decode_frame(None, 0, &mut pcm_transition, f5.min(audiosize), false);
        }

        if let Some(bw) = bandwidth {
            self.celt.set_end_band(match bw {
                Bandwidth::Narrowband => 13,
                Bandwidth::Mediumband | Bandwidth::Wideband => 17,
                Bandwidth::Superwideband => 19,
                Bandwidth::Fullband => 21,
            });
        }
        self.celt.set_stream_channels(self.stream_channels);

        let len = len.max(0) as usize;
        let redundancy_bytes = redundancy_bytes.max(0) as usize;
        // The redundant frame's bytes, after the frame's own.
        let redundant = data.map(|d| &d[len..len + redundancy_bytes]);
        let mut redundant_audio = vec![0i16; if redundancy { f5 * ch } else { 0 }];
        let mut redundant_rng = 0u32;
        // The 5 ms redundant frame from CELT to SILK.
        if redundancy && celt_to_silk {
            // Decoded even where the previous frame was not CELT (and its
            // audio not used): the final range needs it.
            self.celt.set_start_band(0);
            // Its errors, as libopus's ignored return, leave what it wrote.
            let _ = self
                .celt
                .decode(redundant, &mut redundant_audio, f5, None, false);
            redundant_rng = self.celt.final_range();
        }
        // After the concealment above.
        self.celt.set_start_band(start_band);

        let mut celt_ret = Ok(0);
        if mode != Mode::Silk {
            let celt_frame_size = f20.min(frame_size);
            // A change of mode discards CELT's state.
            if Some(mode) != self.prev_mode && self.prev_mode.is_some() && !self.prev_redundancy {
                self.celt.reset();
            }
            let celt_data = if decode_fec {
                None
            } else {
                data.map(|d| &d[..len])
            };
            celt_ret =
                self.celt
                    .decode(celt_data, pcm, celt_frame_size, Some(&mut dec), celt_accum);
        } else {
            if !celt_accum {
                pcm[..frame_size * ch].fill(0);
            }
            // From hybrid to SILK, CELT's MDCT fades out over a silent
            // frame.
            if self.prev_mode == Some(Mode::Hybrid)
                && !(redundancy && celt_to_silk && self.prev_redundancy)
            {
                self.celt.set_start_band(0);
                // A silent frame; its result is the fade, whatever it says.
                let _ = self
                    .celt
                    .decode(Some(&[0xFF, 0xFF]), pcm, f2_5, None, celt_accum);
            }
        }

        if mode != Mode::Celt && !celt_accum {
            for (p, &s) in pcm[..frame_size * ch].iter_mut().zip(&pcm_silk) {
                *p = saturate16(i32::from(*p) + i32::from(s)) as i16;
            }
        }

        // The 5 ms redundant frame from SILK to CELT.
        if redundancy && !celt_to_silk {
            self.celt.reset();
            self.celt.set_start_band(0);
            // As above.
            let _ = self
                .celt
                .decode(redundant, &mut redundant_audio, f5, None, false);
            redundant_rng = self.celt.final_range();
            let at = ch * (frame_size - f2_5);
            let in1 = pcm[at..at + f2_5 * ch].to_vec();
            smooth_fade(
                &in1,
                &redundant_audio[ch * f2_5..],
                &mut pcm[at..],
                f2_5,
                ch,
                fs,
            );
        }
        // The 5 ms redundant frame from CELT to SILK -- unless the previous
        // frame did not use CELT (the transition's first redundant frame
        // lost).
        if redundancy
            && celt_to_silk
            && (self.prev_mode != Some(Mode::Silk) || self.prev_redundancy)
        {
            pcm[..f2_5 * ch].copy_from_slice(&redundant_audio[..f2_5 * ch]);
            let in2 = pcm[ch * f2_5..ch * f2_5 * 2].to_vec();
            smooth_fade(
                &redundant_audio[ch * f2_5..],
                &in2,
                &mut pcm[ch * f2_5..],
                f2_5,
                ch,
                fs,
            );
        }
        if transition {
            if audiosize >= f5 {
                pcm[..ch * f2_5].copy_from_slice(&pcm_transition[..ch * f2_5]);
                let in2 = pcm[ch * f2_5..ch * f2_5 * 2].to_vec();
                smooth_fade(
                    &pcm_transition[ch * f2_5..],
                    &in2,
                    &mut pcm[ch * f2_5..],
                    f2_5,
                    ch,
                    fs,
                );
            } else {
                // Not time enough for a clean transition: the best there is,
                // if not amplitude-preserving.
                let in2 = pcm[..ch * f2_5].to_vec();
                smooth_fade(&pcm_transition, &in2, pcm, f2_5, ch, fs);
            }
        }

        if self.decode_gain != 0 {
            #[allow(clippy::excessive_precision, reason = "libopus's literal, verbatim")]
            let gain = exp2(mult16_16_p15(
                qconst16(6.488_140_81e-4, 25),
                self.decode_gain,
            ));
            for p in &mut pcm[..frame_size * ch] {
                *p = saturate(mult16_32_p16(i32::from(*p), gain), 32767) as i16;
            }
        }

        self.range_final = if len <= 1 {
            0
        } else {
            dec.range() ^ redundant_rng
        };
        self.prev_mode = Some(mode);
        self.prev_redundancy = redundancy && !celt_to_silk;
        celt_ret.map(|_| audiosize)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]
mod tests {
    use super::*;

    /// What `opus_decoder_create`, `OPUS_SET_GAIN`, `opus_decode` and
    /// `opus_decode_float` refuse, refused here too, with libopus's codes.
    #[test]
    fn arguments_libopus_refuses_are_refused() {
        for (rate, channels) in [(44100, 1), (48000, 0), (48000, 3), (0, 2)] {
            assert_eq!(
                Decoder::new(rate, channels).err(),
                Some(Error::BadArgument),
                "{rate} {channels}"
            );
        }
        let mut d = Decoder::new(24000, 2).unwrap();
        assert_eq!((d.sample_rate(), d.channels(), d.gain()), (24000, 2, 0));
        assert_eq!(d.set_gain(32768), Err(Error::BadArgument));
        assert_eq!(d.set_gain(-32769), Err(Error::BadArgument));
        d.set_gain(-300).unwrap();
        assert_eq!(d.gain(), -300);
        // Room for no samples.
        assert_eq!(d.decode(None, &mut [], false), Err(Error::BadArgument));
        assert_eq!(
            d.decode_float(None, &mut [], false),
            Err(Error::BadArgument)
        );
        // Code 3 with no frames: `opus_decode_float` asks the packet's
        // length first, and a packet of no samples is not one.
        let mut f = vec![0f32; 5760 * 2];
        assert_eq!(
            d.decode_float(Some(&[0x03, 0x00]), &mut f, false),
            Err(Error::InvalidPacket)
        );
    }
}
