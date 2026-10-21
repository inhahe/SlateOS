//! A stream's configuration as the decoder takes it up (`aacdec.c`): the
//! AudioSpecificConfig of its global header or a new one in a packet's side
//! data, with its General Audio part, its ELD part and its program config
//! element; an ADTS frame's header (`adts_header.c`); and what a decoder
//! opened with no configuration makes of the channels it is given.
//!
//! Each reads into the configuration in force (`oc[1]`), configuring the
//! output as soon as it knows the channels -- partway, as FFmpeg's does, so
//! that a read failing after it leaves the output configured.

#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's ints: fields of at most 24 bits, and bit counts under i32::MAX"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "element counts of at most 15 each, and lengths of at most 2^16 bytes, in i32"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "a program config element's counts are of 2 to 4 bits: at most 63 of a layout \
              map's 256 entries"
)]

use ffbits::Bits;

use crate::Error;
use crate::context::{Context, EF_EXPLODE, OcStatus};
use crate::layout::{
    CHANNEL_BACK, CHANNEL_CC, CHANNEL_FRONT, CHANNEL_LFE, CHANNEL_SIDE, LayoutMap, MAX_CHANNELS,
    MAX_ELEM_ID, TYPE_CCE, TYPE_LFE, count_channels,
};
use crate::mpeg4audio::{CHANNELS, SAMPLE_RATES, aot, read_config};

/// What an ADTS frame's header says (`AACADTSHeaderInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AdtsHeader {
    pub(crate) sample_rate: i32,
    pub(crate) samples: i32,
    pub(crate) bit_rate: i32,
    pub(crate) crc_absent: i32,
    pub(crate) object_type: i32,
    pub(crate) sampling_index: i32,
    pub(crate) chan_config: i32,
    pub(crate) num_aac_frames: i32,
    pub(crate) frame_length: i32,
}

/// Why an ADTS header was refused (`AAC_PARSE_ERROR_*`): no sync word, a
/// sample rate index with no rate, or a frame shorter than its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdtsError {
    Sync,
    SampleRate,
    FrameSize,
}

/// An ADTS frame's header from where `gb` stands (`ff_adts_header_parse`):
/// the header, its frame's length in bytes among its fields.
///
/// # Errors
///
/// [`AdtsError`], as FFmpeg refuses the header.
pub(crate) fn adts_header_parse(gb: &mut Bits<'_>) -> Result<AdtsHeader, AdtsError> {
    if gb.bits(12) != 0xfff {
        return Err(AdtsError::Sync);
    }
    gb.skip(1); // id
    gb.skip(2); // layer
    let crc_absent = gb.bit() as i32;
    let object_type = gb.bits(2) as i32;
    let sr = gb.bits(4) as i32;
    let sample_rate = SAMPLE_RATES.get(sr as usize).copied().unwrap_or(0);
    if sample_rate == 0 {
        return Err(AdtsError::SampleRate);
    }
    gb.skip(1); // private_bit
    let chan_config = gb.bits(3) as i32;
    gb.skip(1); // original/copy
    gb.skip(1); // home
    gb.skip(1); // copyright_identification_bit
    gb.skip(1); // copyright_identification_start
    let size = gb.bits(13) as i32;
    if size < 7 {
        return Err(AdtsError::FrameSize);
    }
    gb.skip(11); // adts_buffer_fullness
    let rdb = gb.bits(2) as i32;
    let samples = (rdb + 1) * 1024;
    Ok(AdtsHeader {
        sample_rate,
        samples,
        bit_rate: size * 8 * sample_rate / samples,
        crc_absent,
        object_type: object_type + 1,
        sampling_index: sr,
        chan_config,
        num_aac_frames: rdb + 1,
        frame_length: size,
    })
}

/// FFmpeg's sample rate index for a rate no configuration gives an index
/// for: the nearest of the standard's (`ff_aac_sample_rate_idx`).
pub(crate) fn sample_rate_idx(rate: i32) -> i32 {
    const BOUNDS: [i32; 11] = [
        92017, 75132, 55426, 46009, 37566, 27713, 23004, 18783, 13856, 11502, 9391,
    ];
    BOUNDS
        .iter()
        .position(|&b| b <= rate)
        .map_or(11, |i| i as i32)
}

/// `n` elements of position `position` into `layout_map`, each a 4-bit id
/// after its type where the position has more than one
/// (`decode_channel_map`): a front, side or back element is a single or a
/// pair by a bit, a coupling channel spends a bit on nothing, an LFE is an
/// LFE.
fn decode_channel_map(layout_map: &mut [[u8; 3]], position: u8, gb: &mut Bits<'_>, n: usize) {
    for entry in layout_map.iter_mut().take(n) {
        let syn_ele = match position {
            CHANNEL_FRONT | CHANNEL_BACK | CHANNEL_SIDE => gb.bit() as u8,
            CHANNEL_CC => {
                gb.skip(1);
                TYPE_CCE
            }
            _ => TYPE_LFE,
        };
        *entry = [syn_ele, gb.bits(4) as u8, position];
    }
}

/// Skip to the next byte boundary counted from bit `reference_position`
/// (`relative_align_get_bits`).
fn relative_align_get_bits(gb: &mut Bits<'_>, reference_position: i32) {
    let n = reference_position.wrapping_sub(gb.count() as i32) & 7;
    if n != 0 {
        gb.skip(n as u32);
    }
}

/// A program config element into `layout_map` (`decode_pce`): its
/// elements, front, side, back, LFE, then coupling channels, and how many
/// there are; its byte alignment counted from bit `byte_align_ref`.
///
/// # Errors
///
/// [`Error::InvalidData`] where the element runs past the data.
pub(crate) fn decode_pce(
    layout_map: &mut LayoutMap,
    gb: &mut Bits<'_>,
    byte_align_ref: i32,
) -> Result<usize, Error> {
    gb.skip(2); // object_type
    // The sample rate index, which FFmpeg only warns about when it differs
    // from the configuration's.
    gb.skip(4);
    let num_front = gb.bits(4) as usize;
    let num_side = gb.bits(4) as usize;
    let num_back = gb.bits(4) as usize;
    let num_lfe = gb.bits(2) as usize;
    let num_assoc_data = gb.bits(3) as usize;
    let num_cc = gb.bits(4) as usize;

    if gb.bit() != 0 {
        gb.skip(4); // mono_mixdown_tag
    }
    if gb.bit() != 0 {
        gb.skip(4); // stereo_mixdown_tag
    }
    if gb.bit() != 0 {
        gb.skip(3); // mixdown_coeff_index and pseudo_surround
    }

    let needed =
        5 * (num_front + num_side + num_back + num_cc) + 4 * (num_lfe + num_assoc_data + num_cc);
    if i64::from(gb.left()) < needed as i64 {
        // "decode_pce: Input buffer exhausted before END element found".
        return Err(Error::InvalidData);
    }
    let mut tags = 0;
    for (position, n) in [
        (CHANNEL_FRONT, num_front),
        (CHANNEL_SIDE, num_side),
        (CHANNEL_BACK, num_back),
        (CHANNEL_LFE, num_lfe),
    ] {
        decode_channel_map(&mut layout_map[tags..], position, gb, n);
        tags += n;
    }
    gb.skip_long(4 * num_assoc_data as i32);
    decode_channel_map(&mut layout_map[tags..], CHANNEL_CC, gb, num_cc);
    tags += num_cc;

    relative_align_get_bits(gb, byte_align_ref);

    // The comment field: its length in bytes, then the bytes.
    let comment_len = gb.bits(8) as i32 * 8;
    if gb.left() < comment_len {
        return Err(Error::InvalidData);
    }
    gb.skip_long(comment_len);
    Ok(tags)
}

impl Context {
    /// The General Audio part of an AudioSpecificConfig, into the
    /// configuration in force, whose common part is read; the output
    /// configured for its channels -- channel configuration
    /// `channel_config`'s, or a program config element's
    /// (`decode_ga_specific_config`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for a channel configuration or program config
    /// element that cannot be read; [`Error::Unsupported`] for error
    /// resilience tools FFmpeg does not decode.
    fn decode_ga_specific_config(
        &mut self,
        gb: &mut Bits<'_>,
        get_bit_alignment: i32,
        channel_config: i32,
    ) -> Result<(), Error> {
        let mut layout_map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
        let m4ac = &mut self.oc[1].m4ac;
        m4ac.frame_length_short = gb.bit() as i32;
        if m4ac.frame_length_short != 0 && m4ac.sbr == 1 {
            // "SBR with 960 frame length", which FFmpeg does not decode:
            // the core alone.
            m4ac.sbr = 0;
            m4ac.ps = 0;
        }
        if gb.bit() != 0 {
            gb.skip(14); // dependsOnCoreCoder: coreCoderDelay
        }
        let extension_flag = gb.bit();
        if m4ac.object_type == aot::AAC_SCALABLE || m4ac.object_type == aot::ER_AAC_SCALABLE {
            gb.skip(3); // layerNr
        }

        let tags = if channel_config == 0 {
            gb.skip(4); // element_instance_tag
            decode_pce(&mut layout_map, gb, get_bit_alignment)?
        } else {
            self.default_channel_config(&mut layout_map, channel_config)?
        };

        let m4ac = &mut self.oc[1].m4ac;
        if count_channels(&layout_map, tags) > 1 {
            m4ac.ps = 0;
        } else if m4ac.sbr == 1 && m4ac.ps == -1 {
            m4ac.ps = 1;
        }

        self.output_configure(Some(&layout_map), tags, OcStatus::GlobalHdr, false)?;

        let object_type = self.oc[1].m4ac.object_type;
        let error_resilient = matches!(
            object_type,
            aot::ER_AAC_LC | aot::ER_AAC_LTP | aot::ER_AAC_SCALABLE | aot::ER_AAC_LD
        );
        if extension_flag != 0 {
            if object_type == aot::ER_BSAC {
                gb.skip(5); // numOfSubFrame
                gb.skip(11); // layer_length
            } else if error_resilient && gb.bits(3) != 0 {
                // "AAC data resilience": its resilience flags.
                return Err(Error::Unsupported);
            }
            gb.skip(1); // extensionFlag3
        }
        if error_resilient && gb.bits(2) != 0 {
            // An epConfig other than 0.
            return Err(Error::Unsupported);
        }
        Ok(())
    }

    /// The ELD part of an AudioSpecificConfig, into the configuration in
    /// force; the output configured for channel configuration
    /// `channel_config` (`decode_eld_specific_config`).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for error resilience tools or low-delay SBR,
    /// which FFmpeg does not decode; [`Error::InvalidData`] for an extension
    /// running past the data, or a channel configuration with no layout.
    fn decode_eld_specific_config(
        &mut self,
        gb: &mut Bits<'_>,
        channel_config: i32,
    ) -> Result<(), Error> {
        let m4ac = &mut self.oc[1].m4ac;
        m4ac.ps = 0;
        m4ac.sbr = 0;
        m4ac.frame_length_short = gb.bit() as i32;
        if gb.bits(3) != 0 {
            // "AAC data resilience".
            return Err(Error::Unsupported);
        }
        if gb.bit() != 0 {
            // ldSbrPresentFlag: "Low Delay SBR".
            return Err(Error::Unsupported);
        }
        // Extensions until ELDEXT_TERM, each passed over.
        while gb.bits(4) != 0 {
            let mut len = gb.bits(4) as i32;
            if len == 15 {
                len += gb.bits(8) as i32;
            }
            if len == 15 + 255 {
                len += gb.bits(16) as i32;
            }
            if gb.left() < len * 8 + 4 {
                return Err(Error::InvalidData);
            }
            gb.skip_long(8 * len);
        }
        let mut layout_map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
        let tags = self.default_channel_config(&mut layout_map, channel_config)?;
        self.output_configure(Some(&layout_map), tags, OcStatus::GlobalHdr, false)?;
        if gb.bits(2) != 0 {
            // An epConfig other than 0.
            return Err(Error::Unsupported);
        }
        Ok(())
    }

    /// An AudioSpecificConfig from where `gb` stands, into the
    /// configuration in force (`decode_audio_specific_config_gb`): its
    /// common part, then its object type's own, which configures the
    /// output; with `sync_extension`, a backward-compatible SBR and PS
    /// signal looked for after it. A program config element in it is byte
    /// aligned from bit `get_bit_alignment`. Answers the bits read.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for a config that cannot be read, which leaves
    /// the common part as it was; [`Error::Unsupported`] for an object type
    /// FFmpeg's fixed-point decoder does not decode -- USAC among them,
    /// which FFmpeg reads here and refuses frame by frame.
    pub(crate) fn decode_audio_specific_config_gb(
        &mut self,
        gb: &mut Bits<'_>,
        get_bit_alignment: i32,
        sync_extension: bool,
    ) -> Result<u32, Error> {
        // The common part is read from a copy; the config's own part from
        // where the common part ends, before any sync extension.
        let mut gbc = gb.clone();
        let backup = self.oc[1].m4ac;
        let m4ac = &mut self.oc[1].m4ac;
        let Ok(specific) = read_config(m4ac, &mut gbc, sync_extension) else {
            *m4ac = backup;
            return Err(Error::InvalidData);
        };
        if m4ac.sampling_index > 12
            || (m4ac.object_type == aot::ER_AAC_LD
                && (m4ac.sampling_index < 3 || m4ac.sampling_index > 7))
        {
            *m4ac = backup;
            return Err(Error::InvalidData);
        }
        gb.skip_long(specific);

        let (object_type, chan_config) = (m4ac.object_type, m4ac.chan_config);
        match object_type {
            aot::AAC_MAIN
            | aot::AAC_LC
            | aot::AAC_SSR
            | aot::AAC_LTP
            | aot::ER_AAC_LC
            | aot::ER_AAC_LD => {
                self.decode_ga_specific_config(gb, get_bit_alignment, chan_config)?;
            }
            aot::ER_AAC_ELD => self.decode_eld_specific_config(gb, chan_config)?,
            // USAC, which FFmpeg's fixed-point decoder refuses, and the
            // object types no decoder of FFmpeg's has.
            _ => return Err(Error::Unsupported),
        }
        Ok(gb.count())
    }

    /// An AudioSpecificConfig of `bit_size` bits in `data`
    /// (`decode_audio_specific_config`): a global header, or a new one from
    /// a packet's side data.
    ///
    /// # Errors
    ///
    /// As [`Self::decode_audio_specific_config_gb`]; [`Error::InvalidData`]
    /// for a size past what FFmpeg's reader takes.
    pub(crate) fn decode_audio_specific_config(
        &mut self,
        data: &[u8],
        bit_size: usize,
        sync_extension: bool,
    ) -> Result<u32, Error> {
        if bit_size > i32::MAX as usize {
            return Err(Error::InvalidData);
        }
        let mut gb = Bits::new(data, bit_size).ok_or(Error::InvalidData)?;
        self.decode_audio_specific_config_gb(&mut gb, 0, sync_extension)
    }

    /// The configuration a decoder opens with (`ff_aac_decode_init`, less
    /// its transforms): from `extradata`'s AudioSpecificConfig where there
    /// is one, otherwise from the codec context's sample rate and channel
    /// count alone.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for a sample rate over 96 kHz, more channels
    /// than [`MAX_CHANNELS`], or a configuration that cannot be read;
    /// [`Error::Unsupported`] for one FFmpeg's fixed-point decoder does not
    /// decode.
    pub(crate) fn init_config(&mut self, extradata: &[u8]) -> Result<(), Error> {
        if self.avctx.sample_rate > 96000 {
            return Err(Error::InvalidData);
        }
        self.oc[1].m4ac.sample_rate = self.avctx.sample_rate;
        if extradata.is_empty() {
            let channels = self.avctx.ch_layout.nb_channels;
            let m4ac = &mut self.oc[1].m4ac;
            m4ac.sampling_index = sample_rate_idx(self.avctx.sample_rate);
            m4ac.channels = channels;
            m4ac.sbr = -1;
            m4ac.ps = -1;
            // The first configuration with that many channels, 0 for none.
            m4ac.chan_config = CHANNELS
                .iter()
                .position(|&n| i32::from(n) == channels)
                .map_or(0, |i| i as i32);
            let chan_config = m4ac.chan_config;
            if chan_config != 0 {
                let mut layout_map: LayoutMap = [[0; 3]; MAX_ELEM_ID * 4];
                match self.default_channel_config(&mut layout_map, chan_config) {
                    // FFmpeg does not look at how configuring went.
                    Ok(tags) => {
                        let _ = self.output_configure(
                            Some(&layout_map),
                            tags,
                            OcStatus::GlobalHdr,
                            false,
                        );
                    }
                    Err(e) if self.avctx.err_recognition & EF_EXPLODE != 0 => return Err(e),
                    Err(_) => {}
                }
            }
        } else {
            self.decode_audio_specific_config(extradata, extradata.len().saturating_mul(8), true)?;
        }
        if usize::try_from(self.avctx.ch_layout.nb_channels).unwrap_or(0) > MAX_CHANNELS {
            // "Too many channels".
            return Err(Error::InvalidData);
        }
        self.random_state = 0x1f2e_3d4c;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
