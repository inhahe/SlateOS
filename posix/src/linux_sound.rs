//! `<linux/sound.h>` + `<linux/soundcard.h>` — sound device constants.
//!
//! Defines OSS (Open Sound System) compatible ioctl commands and
//! ALSA card/device type identifiers used for audio device management.

// ---------------------------------------------------------------------------
// OSS ioctl commands
// ---------------------------------------------------------------------------

/// Get DSP capabilities.
pub const SNDCTL_DSP_GETCAPS: u64 = 0x80045012;
/// Reset DSP.
pub const SNDCTL_DSP_RESET: u64 = 0x00005000;
/// Sync DSP.
pub const SNDCTL_DSP_SYNC: u64 = 0x00005001;
/// Set fragment size.
pub const SNDCTL_DSP_SETFRAGMENT: u64 = 0xC004500A;
/// Set sample format.
pub const SNDCTL_DSP_SETFMT: u64 = 0xC0045005;
/// Set stereo/mono.
pub const SNDCTL_DSP_STEREO: u64 = 0xC0045003;
/// Set channels.
pub const SNDCTL_DSP_CHANNELS: u64 = 0xC0045006;
/// Set sample rate.
pub const SNDCTL_DSP_SPEED: u64 = 0xC0045002;
/// Get output delay.
pub const SNDCTL_DSP_GETODELAY: u64 = 0x80045017;
/// Get output space.
pub const SNDCTL_DSP_GETOSPACE: u64 = 0x8010500C;
/// Get input space.
pub const SNDCTL_DSP_GETISPACE: u64 = 0x8010500D;
/// Non-blocking mode.
pub const SNDCTL_DSP_NONBLOCK: u64 = 0x0000500E;
/// Get pointer.
pub const SNDCTL_DSP_GETOPTR: u64 = 0x800C5012;
/// Get input pointer.
pub const SNDCTL_DSP_GETIPTR: u64 = 0x800C5011;

// ---------------------------------------------------------------------------
// OSS audio formats
// ---------------------------------------------------------------------------

/// Unsigned 8-bit.
pub const AFMT_U8: u32 = 0x00000008;
/// Signed 16-bit little-endian.
pub const AFMT_S16_LE: u32 = 0x00000010;
/// Signed 16-bit big-endian.
pub const AFMT_S16_BE: u32 = 0x00000020;
/// Signed 8-bit.
pub const AFMT_S8: u32 = 0x00000040;
/// Unsigned 16-bit little-endian.
pub const AFMT_U16_LE: u32 = 0x00000080;
/// Unsigned 16-bit big-endian.
pub const AFMT_U16_BE: u32 = 0x00000100;
/// IMA ADPCM.
pub const AFMT_IMA_ADPCM: u32 = 0x00000004;
/// Mu-law encoding.
pub const AFMT_MU_LAW: u32 = 0x00000001;
/// A-law encoding.
pub const AFMT_A_LAW: u32 = 0x00000002;
/// MPEG audio.
pub const AFMT_MPEG: u32 = 0x00000200;
/// AC3 audio.
pub const AFMT_AC3: u32 = 0x00000400;
/// Signed 32-bit little-endian.
pub const AFMT_S32_LE: u32 = 0x00001000;
/// Signed 32-bit big-endian.
pub const AFMT_S32_BE: u32 = 0x00002000;
/// Signed 24-bit little-endian.
pub const AFMT_S24_LE: u32 = 0x00008000;
/// Signed 24-bit big-endian.
pub const AFMT_S24_BE: u32 = 0x00010000;

// ---------------------------------------------------------------------------
// Mixer ioctls
// ---------------------------------------------------------------------------

/// Read volume.
pub const SOUND_MIXER_READ_VOLUME: u64 = 0x80044D00;
/// Write volume.
pub const SOUND_MIXER_WRITE_VOLUME: u64 = 0xC0044D00;
/// Read PCM volume.
pub const SOUND_MIXER_READ_PCM: u64 = 0x80044D04;
/// Write PCM volume.
pub const SOUND_MIXER_WRITE_PCM: u64 = 0xC0044D04;
/// Read device mask.
pub const SOUND_MIXER_READ_DEVMASK: u64 = 0x80044DFE;
/// Read record mask.
pub const SOUND_MIXER_READ_RECMASK: u64 = 0x80044DFD;
/// Read stereo devs.
pub const SOUND_MIXER_READ_STEREODEVS: u64 = 0x80044DFB;
/// Read capabilities.
pub const SOUND_MIXER_READ_CAPS: u64 = 0x80044DFC;

// ---------------------------------------------------------------------------
// ALSA card types (SNDRV_*)
// ---------------------------------------------------------------------------

/// PCM device type.
pub const SNDRV_DEV_TYPE_PCM: i32 = 0;
/// Control device type.
pub const SNDRV_DEV_TYPE_CONTROL: i32 = 1;
/// Raw MIDI device type.
pub const SNDRV_DEV_TYPE_RAWMIDI: i32 = 2;
/// Timer device type.
pub const SNDRV_DEV_TYPE_TIMER: i32 = 3;
/// Sequencer device type.
pub const SNDRV_DEV_TYPE_SEQUENCER: i32 = 4;
/// Hardware-dependent device.
pub const SNDRV_DEV_TYPE_HWDEP: i32 = 5;

// ---------------------------------------------------------------------------
// ALSA PCM stream direction
// ---------------------------------------------------------------------------

/// Playback stream.
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream.
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// ALSA PCM formats
// ---------------------------------------------------------------------------

/// Signed 8-bit.
pub const SNDRV_PCM_FORMAT_S8: i32 = 0;
/// Unsigned 8-bit.
pub const SNDRV_PCM_FORMAT_U8: i32 = 1;
/// Signed 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_S16_LE: i32 = 2;
/// Signed 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_S16_BE: i32 = 3;
/// Unsigned 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_U16_LE: i32 = 4;
/// Unsigned 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_U16_BE: i32 = 5;
/// Signed 24-bit little-endian (3 bytes).
pub const SNDRV_PCM_FORMAT_S24_LE: i32 = 6;
/// Signed 24-bit big-endian (3 bytes).
pub const SNDRV_PCM_FORMAT_S24_BE: i32 = 7;
/// Signed 32-bit little-endian.
pub const SNDRV_PCM_FORMAT_S32_LE: i32 = 10;
/// Signed 32-bit big-endian.
pub const SNDRV_PCM_FORMAT_S32_BE: i32 = 11;
/// 32-bit float little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_LE: i32 = 14;
/// 32-bit float big-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_BE: i32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_formats_distinct() {
        let fmts = [
            AFMT_MU_LAW, AFMT_A_LAW, AFMT_IMA_ADPCM, AFMT_U8,
            AFMT_S16_LE, AFMT_S16_BE, AFMT_S8, AFMT_U16_LE,
            AFMT_U16_BE, AFMT_MPEG, AFMT_AC3,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_oss_formats_powers_of_two() {
        let fmts = [
            AFMT_MU_LAW, AFMT_A_LAW, AFMT_IMA_ADPCM, AFMT_U8,
            AFMT_S16_LE, AFMT_S16_BE, AFMT_S8, AFMT_U16_LE,
            AFMT_U16_BE, AFMT_MPEG, AFMT_AC3,
        ];
        for f in &fmts {
            assert!(f.is_power_of_two(), "format {f:#x} not power of 2");
        }
    }

    #[test]
    fn test_alsa_pcm_formats_sequential() {
        assert_eq!(SNDRV_PCM_FORMAT_S8, 0);
        assert_eq!(SNDRV_PCM_FORMAT_U8, 1);
        assert_eq!(SNDRV_PCM_FORMAT_S16_LE, 2);
        assert_eq!(SNDRV_PCM_FORMAT_S16_BE, 3);
    }

    #[test]
    fn test_dev_types_distinct() {
        let types = [
            SNDRV_DEV_TYPE_PCM, SNDRV_DEV_TYPE_CONTROL,
            SNDRV_DEV_TYPE_RAWMIDI, SNDRV_DEV_TYPE_TIMER,
            SNDRV_DEV_TYPE_SEQUENCER, SNDRV_DEV_TYPE_HWDEP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pcm_streams() {
        assert_eq!(SNDRV_PCM_STREAM_PLAYBACK, 0);
        assert_eq!(SNDRV_PCM_STREAM_CAPTURE, 1);
    }

    #[test]
    fn test_mixer_ioctls_distinct() {
        let ioctls = [
            SOUND_MIXER_READ_VOLUME, SOUND_MIXER_WRITE_VOLUME,
            SOUND_MIXER_READ_PCM, SOUND_MIXER_WRITE_PCM,
            SOUND_MIXER_READ_DEVMASK, SOUND_MIXER_READ_RECMASK,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
