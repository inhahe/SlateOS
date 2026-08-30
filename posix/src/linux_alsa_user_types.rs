//! `<sound/asound.h>` — ALSA device-node paths and protocol versions.
//!
//! Userspace opens ALSA devices through `/dev/snd/`. This module
//! collects the path conventions and the protocol-version constants
//! returned by `SNDRV_*_IOCTL_PVERSION` ioctls. The core
//! `snd_device_type` enum is in `linux_alsa_types`.

// ---------------------------------------------------------------------------
// Device-node directory and per-device naming
// ---------------------------------------------------------------------------

pub const DEV_SND: &str = "/dev/snd";

pub const SND_CTL_PREFIX: &str = "controlC"; // controlC<card>
pub const SND_PCM_P_PREFIX: &str = "pcmC"; // pcmC<card>D<dev>p
pub const SND_PCM_C_SUFFIX_P: u8 = b'p';
pub const SND_PCM_C_SUFFIX_C: u8 = b'c';
pub const SND_MIDI_PREFIX: &str = "midiC";
pub const SND_HWDEP_PREFIX: &str = "hwC";
pub const SND_TIMER_NODE: &str = "timer";
pub const SND_SEQ_NODE: &str = "seq";

// ---------------------------------------------------------------------------
// Maximum card and device counts
// ---------------------------------------------------------------------------

pub const SNDRV_CARDS: u32 = 32;
pub const SNDRV_PCM_DEVICES: u32 = 8;

// ---------------------------------------------------------------------------
// Protocol versions (`SNDRV_*_IOCTL_PVERSION`)
// ---------------------------------------------------------------------------

#[must_use]
pub const fn snd_protocol_version(major: u8, minor: u8, patch: u16) -> u32 {
    ((major as u32) << 16) | ((minor as u32) << 8) | (patch as u32)
}

pub const SNDRV_CTL_VERSION: u32 = snd_protocol_version(2, 0, 8);
pub const SNDRV_PCM_VERSION: u32 = snd_protocol_version(2, 0, 17);
pub const SNDRV_RAWMIDI_VERSION: u32 = snd_protocol_version(2, 0, 2);
pub const SNDRV_HWDEP_VERSION: u32 = snd_protocol_version(1, 0, 1);
pub const SNDRV_TIMER_VERSION: u32 = snd_protocol_version(2, 0, 7);

// ---------------------------------------------------------------------------
// /proc/asound paths
// ---------------------------------------------------------------------------

pub const PROC_ASOUND: &str = "/proc/asound";
pub const PROC_ASOUND_CARDS: &str = "/proc/asound/cards";
pub const PROC_ASOUND_VERSION: &str = "/proc/asound/version";
pub const PROC_ASOUND_DEVICES: &str = "/proc/asound/devices";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_snd_paths() {
        assert_eq!(DEV_SND, "/dev/snd");
        assert!(SND_CTL_PREFIX.starts_with("control"));
        assert!(SND_PCM_P_PREFIX.starts_with("pcm"));
        assert_ne!(SND_PCM_C_SUFFIX_P, SND_PCM_C_SUFFIX_C);
        assert_eq!(SND_PCM_C_SUFFIX_P, b'p');
        assert_eq!(SND_PCM_C_SUFFIX_C, b'c');
    }

    #[test]
    fn test_card_and_device_caps_powers_of_two() {
        assert!(SNDRV_CARDS.is_power_of_two());
        assert_eq!(SNDRV_CARDS, 32);
        assert!(SNDRV_PCM_DEVICES.is_power_of_two());
        assert_eq!(SNDRV_PCM_DEVICES, 8);
    }

    #[test]
    fn test_protocol_version_encoding() {
        // Major in high byte (bits 16..24), minor middle (8..16), patch low (0..16).
        let v = snd_protocol_version(2, 0, 17);
        assert_eq!(v >> 16, 2);
        assert_eq!((v >> 8) & 0xFF, 0);
        assert_eq!(v & 0xFFFF, 17);
        // PCM is the most-rev'd interface.
        assert_eq!(SNDRV_PCM_VERSION, 0x0002_0011);
        // HWDEP is at v1.0.x — older API.
        assert_eq!(SNDRV_HWDEP_VERSION >> 16, 1);
    }

    #[test]
    fn test_protocol_versions_all_v2_except_hwdep() {
        for v in [
            SNDRV_CTL_VERSION,
            SNDRV_PCM_VERSION,
            SNDRV_RAWMIDI_VERSION,
            SNDRV_TIMER_VERSION,
        ] {
            assert_eq!(v >> 16, 2);
        }
        assert_eq!(SNDRV_HWDEP_VERSION >> 16, 1);
    }

    #[test]
    fn test_proc_asound_paths() {
        assert!(PROC_ASOUND_CARDS.starts_with(PROC_ASOUND));
        assert!(PROC_ASOUND_VERSION.starts_with(PROC_ASOUND));
        assert!(PROC_ASOUND_DEVICES.starts_with(PROC_ASOUND));
    }
}
