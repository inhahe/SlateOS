//! `<sound/firewire.h>` — ALSA FireWire audio constants.
//!
//! FireWire (IEEE 1394) audio devices use isochronous streaming
//! for low-latency, multi-channel audio. Common protocols include
//! AMDTP (Audio and Music Data Transmission Protocol) used by most
//! pro audio interfaces, and vendor-specific protocols (MOTU, RME,
//! DICE). The ALSA FireWire stack handles device discovery, stream
//! management, and clock synchronization.

// ---------------------------------------------------------------------------
// FireWire audio IOCTLs
// ---------------------------------------------------------------------------

/// Get device information.
pub const SNDRV_FIREWIRE_IOCTL_GET_INFO: u32 = 0x00;
/// Lock the device (exclusive streaming access).
pub const SNDRV_FIREWIRE_IOCTL_LOCK: u32 = 0x01;
/// Unlock the device.
pub const SNDRV_FIREWIRE_IOCTL_UNLOCK: u32 = 0x02;

// ---------------------------------------------------------------------------
// FireWire audio device types
// ---------------------------------------------------------------------------

/// DICE (Digital Interface Communication Engine) device.
pub const SNDRV_FIREWIRE_TYPE_DICE: u32 = 1;
/// Fireworks (Echo Audio) device.
pub const SNDRV_FIREWIRE_TYPE_FIREWORKS: u32 = 2;
/// BeBoB (BridgeCo) device.
pub const SNDRV_FIREWIRE_TYPE_BEBOB: u32 = 3;
/// OXFW (Oxford Semiconductor) device.
pub const SNDRV_FIREWIRE_TYPE_OXFW: u32 = 4;
/// DIGI (Digidesign) device.
pub const SNDRV_FIREWIRE_TYPE_DIGI00X: u32 = 5;
/// TASCAM device.
pub const SNDRV_FIREWIRE_TYPE_TASCAM: u32 = 6;
/// MOTU device.
pub const SNDRV_FIREWIRE_TYPE_MOTU: u32 = 7;
/// Fireface (RME) device.
pub const SNDRV_FIREWIRE_TYPE_FIREFACE: u32 = 8;

// ---------------------------------------------------------------------------
// AMDTP stream format types
// ---------------------------------------------------------------------------

/// IEC 61883-6 (AM824, standard audio).
pub const AMDTP_FORMAT_AM824: u32 = 0;
/// IEC 61883-6 with MIDI (audio + MIDI multiplexed).
pub const AMDTP_FORMAT_AM824_MIDI: u32 = 1;

// ---------------------------------------------------------------------------
// FireWire audio sampling rates
// ---------------------------------------------------------------------------

/// 32000 Hz.
pub const SNDRV_FW_RATE_32000: u32 = 32000;
/// 44100 Hz.
pub const SNDRV_FW_RATE_44100: u32 = 44100;
/// 48000 Hz.
pub const SNDRV_FW_RATE_48000: u32 = 48000;
/// 88200 Hz.
pub const SNDRV_FW_RATE_88200: u32 = 88200;
/// 96000 Hz.
pub const SNDRV_FW_RATE_96000: u32 = 96000;
/// 176400 Hz.
pub const SNDRV_FW_RATE_176400: u32 = 176400;
/// 192000 Hz.
pub const SNDRV_FW_RATE_192000: u32 = 192000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SNDRV_FIREWIRE_IOCTL_GET_INFO,
            SNDRV_FIREWIRE_IOCTL_LOCK,
            SNDRV_FIREWIRE_IOCTL_UNLOCK,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [
            SNDRV_FIREWIRE_TYPE_DICE, SNDRV_FIREWIRE_TYPE_FIREWORKS,
            SNDRV_FIREWIRE_TYPE_BEBOB, SNDRV_FIREWIRE_TYPE_OXFW,
            SNDRV_FIREWIRE_TYPE_DIGI00X, SNDRV_FIREWIRE_TYPE_TASCAM,
            SNDRV_FIREWIRE_TYPE_MOTU, SNDRV_FIREWIRE_TYPE_FIREFACE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_format_types_distinct() {
        assert_ne!(AMDTP_FORMAT_AM824, AMDTP_FORMAT_AM824_MIDI);
    }

    #[test]
    fn test_rates_distinct_and_ordered() {
        let rates = [
            SNDRV_FW_RATE_32000, SNDRV_FW_RATE_44100,
            SNDRV_FW_RATE_48000, SNDRV_FW_RATE_88200,
            SNDRV_FW_RATE_96000, SNDRV_FW_RATE_176400,
            SNDRV_FW_RATE_192000,
        ];
        for i in 0..rates.len() - 1 {
            assert!(rates[i] < rates[i + 1]);
        }
    }
}
