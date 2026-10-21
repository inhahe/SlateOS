//! The common part of an MPEG-4 AudioSpecificConfig -- object type, sample
//! rate, channel configuration, and the SBR and PS signalling around them
//! -- read as FFmpeg's `mpeg4audio.c` reads it
//! (`ff_mpeg4audio_get_config_gb`).
//!
//! The config is read into a [`Config`] the caller already holds, field by
//! field as FFmpeg writes them: a read that fails partway leaves what it
//! wrote before failing, as FFmpeg's does, and the decoder goes on with
//! that.

use ffbits::Bits;

/// The MPEG-4 audio object types FFmpeg names (`enum AudioObjectType`).
pub(crate) mod aot {
    pub(crate) const NULL: i32 = 0;
    pub(crate) const AAC_MAIN: i32 = 1;
    pub(crate) const AAC_LC: i32 = 2;
    pub(crate) const AAC_SSR: i32 = 3;
    pub(crate) const AAC_LTP: i32 = 4;
    pub(crate) const SBR: i32 = 5;
    pub(crate) const AAC_SCALABLE: i32 = 6;
    pub(crate) const ER_AAC_LC: i32 = 17;
    pub(crate) const ER_AAC_LTP: i32 = 19;
    pub(crate) const ER_AAC_SCALABLE: i32 = 20;
    pub(crate) const ER_BSAC: i32 = 22;
    pub(crate) const ER_AAC_LD: i32 = 23;
    pub(crate) const PS: i32 = 29;
    pub(crate) const ESCAPE: i32 = 31;
    pub(crate) const ALS: i32 = 36;
    pub(crate) const ER_AAC_ELD: i32 = 39;
    pub(crate) const USAC: i32 = 42;
}

/// The sample rates a 4-bit index names; 13 real, the rest 0
/// (`ff_mpeg4audio_sample_rates`).
pub(crate) const SAMPLE_RATES: [i32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

/// The channels each channel configuration has; 0 for a configuration the
/// stream's program config element gives, or none (`ff_mpeg4audio_channels`).
pub(crate) const CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 8, 0, 0, 0, 7, 8, 24, 8];

/// An AudioSpecificConfig's common part (`MPEG4AudioConfig`). The fields
/// are FFmpeg's `int`s, with their meanings: `sbr` and `ps` are -1 when
/// only an implicit signal could say (in the stream itself), 1 when the
/// config signals them, 0 when it rules them out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) object_type: i32,
    pub(crate) sampling_index: i32,
    pub(crate) sample_rate: i32,
    pub(crate) chan_config: i32,
    pub(crate) sbr: i32,
    pub(crate) ext_object_type: i32,
    pub(crate) ext_sampling_index: i32,
    pub(crate) ext_sample_rate: i32,
    pub(crate) channels: i32,
    pub(crate) ps: i32,
    pub(crate) frame_length_short: i32,
}

/// A config FFmpeg refuses (its `AVERROR_INVALIDDATA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Invalid;

/// FFmpeg's `MKBETAG`: four bytes as one big-endian word.
const fn tag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from_be_bytes([a, b, c, d])
}

/// An object type: five bits, or six more after the escape
/// (`get_object_type`).
#[allow(clippy::cast_possible_wrap, reason = "at most 6 bits, as FFmpeg's int")]
fn object_type(gb: &mut Bits<'_>) -> i32 {
    let object_type = gb.bits(5) as i32;
    if object_type == aot::ESCAPE {
        32i32.wrapping_add(gb.bits(6) as i32)
    } else {
        object_type
    }
}

/// A sample rate: a 4-bit index, or 24 bits of rate after the escape index
/// 15; the index read goes into `index` (`get_sample_rate`).
#[allow(
    clippy::cast_possible_wrap,
    reason = "at most 24 bits, as FFmpeg's int"
)]
fn sample_rate(gb: &mut Bits<'_>, index: &mut i32) -> i32 {
    *index = gb.bits(4) as i32;
    if *index == 0x0f {
        gb.bits(24) as i32
    } else {
        SAMPLE_RATES
            .get(usize::try_from(*index).unwrap_or(0))
            .copied()
            .unwrap_or(0)
    }
}

/// An ALS config's own rate and channels, which old ALS conformance files
/// give wrong in the common part (`parse_config_ALS`).
#[allow(
    clippy::cast_possible_wrap,
    reason = "FFmpeg's int: a rate of 2^31 or more is negative and refused"
)]
fn als(gb: &mut Bits<'_>, c: &mut Config) -> Result<(), Invalid> {
    if gb.left() < 112 {
        return Err(Invalid);
    }
    if gb.bits_long(32) != tag(b'A', b'L', b'S', 0) {
        return Err(Invalid);
    }
    c.sample_rate = gb.bits_long(32) as i32;
    if c.sample_rate <= 0 {
        return Err(Invalid);
    }
    // The number of samples.
    gb.skip_long(32);
    c.chan_config = 0;
    c.channels = (gb.bits(16) as i32).wrapping_add(1);
    Ok(())
}

/// Read the common part of an AudioSpecificConfig into `c`
/// (`ff_mpeg4audio_get_config_gb`): `sync_extension` looks past it for a
/// backward-compatible SBR and PS signal. Answers how many bits were read
/// before the object type's own config starts.
///
/// # Errors
///
/// [`Invalid`] for a channel configuration of 15, or an ALS config that is
/// broken -- `c` then holds what was read before.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "bit positions are under i32::MAX, as FFmpeg's ints are"
)]
pub(crate) fn read_config(
    c: &mut Config,
    gb: &mut Bits<'_>,
    sync_extension: bool,
) -> Result<i32, Invalid> {
    let start = gb.count() as i32;
    c.object_type = object_type(gb);
    c.sample_rate = sample_rate(gb, &mut c.sampling_index);
    c.chan_config = gb.bits(4) as i32;
    match CHANNELS.get(c.chan_config as usize) {
        Some(&n) => c.channels = i32::from(n),
        None => return Err(Invalid),
    }
    c.sbr = -1;
    c.ps = -1;
    // A PS object type that is no MP3-on-MP4 draft (W6132 Annex YYYY).
    let ps_explicit =
        c.object_type == aot::PS && !(gb.show(3) & 0x03 != 0 && gb.show(9) & 0x3f == 0);
    if c.object_type == aot::SBR || ps_explicit {
        if c.object_type == aot::PS {
            c.ps = 1;
        }
        c.ext_object_type = aot::SBR;
        c.sbr = 1;
        c.ext_sample_rate = sample_rate(gb, &mut c.ext_sampling_index);
        c.object_type = object_type(gb);
        if c.object_type == aot::ER_BSAC {
            gb.bits(4); // extensionChannelConfiguration, which nothing reads
        }
    } else {
        c.ext_object_type = aot::NULL;
        c.ext_sample_rate = 0;
    }
    let mut specific = gb.count() as i32;

    if c.object_type == aot::ALS {
        gb.skip(5);
        if gb.show(24) != tag(0, b'A', b'L', b'S') {
            gb.skip(24);
        }
        specific = gb.count() as i32;
        als(gb, c)?;
    }

    if c.ext_object_type != aot::SBR && sync_extension {
        while gb.left() > 15 {
            if gb.show(11) == 0x2b7 {
                gb.skip(11);
                c.ext_object_type = object_type(gb);
                if c.ext_object_type == aot::SBR {
                    c.sbr = gb.bit() as i32;
                    if c.sbr == 1 {
                        c.ext_sample_rate = sample_rate(gb, &mut c.ext_sampling_index);
                        if c.ext_sample_rate == c.sample_rate {
                            c.sbr = -1;
                        }
                    }
                }
                if gb.left() > 11 && gb.bits(11) == 0x548 {
                    c.ps = gb.bit() as i32;
                }
                break;
            }
            gb.skip(1);
        }
    }

    // PS requires SBR, and implicit PS is for the HE-AACv2 profile alone.
    if c.sbr == 0 {
        c.ps = 0;
    }
    if (c.ps == -1 && c.object_type != aot::AAC_LC) || c.channels & !0x01 != 0 {
        c.ps = 0;
    }
    Ok(specific.wrapping_sub(start))
}

#[cfg(test)]
mod tests;
