//! `<sound/asound.h>` (hwdep subset) — ALSA hardware-dependent interface constants.
//!
//! The HWDEP (hardware-dependent) interface provides direct access to
//! device-specific features that don't fit the standard PCM/control
//! model: firmware upload, DSP programming, hardware-specific ioctls,
//! and raw register access. Each HWDEP device (/dev/snd/hwC0D0) is
//! associated with a specific sound card and device index.

// ---------------------------------------------------------------------------
// HWDEP interface types (well-known hardware)
// ---------------------------------------------------------------------------

/// OPL2/OPL3 FM synthesizer.
pub const SNDRV_HWDEP_IFACE_OPL2: u32 = 0;
/// OPL3 FM synthesizer.
pub const SNDRV_HWDEP_IFACE_OPL3: u32 = 1;
/// OPL4 (OPL3 + wavetable).
pub const SNDRV_HWDEP_IFACE_OPL4: u32 = 2;
/// Creative SB16 CSP.
pub const SNDRV_HWDEP_IFACE_SB16CSP: u32 = 3;
/// EMU10K1 (Sound Blaster Live/Audigy DSP).
pub const SNDRV_HWDEP_IFACE_EMU10K1: u32 = 4;
/// Yamaha YSS225 (DS-1 synthesizer).
pub const SNDRV_HWDEP_IFACE_YSS225: u32 = 5;
/// ICS2115 (wavetable).
pub const SNDRV_HWDEP_IFACE_ICS2115: u32 = 6;
/// Ensoniq SoundScape.
pub const SNDRV_HWDEP_IFACE_SSCAPE: u32 = 7;
/// Digigram VX.
pub const SNDRV_HWDEP_IFACE_VX: u32 = 8;
/// Digigram MIXART.
pub const SNDRV_HWDEP_IFACE_MIXART: u32 = 9;
/// USB streaming.
pub const SNDRV_HWDEP_IFACE_USX2Y: u32 = 10;
/// Echo Audio Indigo/Layla.
pub const SNDRV_HWDEP_IFACE_ECHOAUDIO: u32 = 11;
/// PCXHR (Digigram).
pub const SNDRV_HWDEP_IFACE_PCXHR: u32 = 12;
/// Firmware upload (generic).
pub const SNDRV_HWDEP_IFACE_FW_LOADER: u32 = 13;
/// HDA codec (Intel HD Audio).
pub const SNDRV_HWDEP_IFACE_HDA: u32 = 15;
/// USB Audio Class streaming.
pub const SNDRV_HWDEP_IFACE_USB_STREAM: u32 = 16;

// ---------------------------------------------------------------------------
// HWDEP open flags
// ---------------------------------------------------------------------------

/// Open for read.
pub const SNDRV_HWDEP_OPEN_READ: u32 = 0x01;
/// Open for write.
pub const SNDRV_HWDEP_OPEN_WRITE: u32 = 0x02;
/// Open non-blocking.
pub const SNDRV_HWDEP_OPEN_NONBLOCK: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifaces_distinct() {
        let ifaces = [
            SNDRV_HWDEP_IFACE_OPL2,
            SNDRV_HWDEP_IFACE_OPL3,
            SNDRV_HWDEP_IFACE_OPL4,
            SNDRV_HWDEP_IFACE_SB16CSP,
            SNDRV_HWDEP_IFACE_EMU10K1,
            SNDRV_HWDEP_IFACE_YSS225,
            SNDRV_HWDEP_IFACE_ICS2115,
            SNDRV_HWDEP_IFACE_SSCAPE,
            SNDRV_HWDEP_IFACE_VX,
            SNDRV_HWDEP_IFACE_MIXART,
            SNDRV_HWDEP_IFACE_USX2Y,
            SNDRV_HWDEP_IFACE_ECHOAUDIO,
            SNDRV_HWDEP_IFACE_PCXHR,
            SNDRV_HWDEP_IFACE_FW_LOADER,
            SNDRV_HWDEP_IFACE_HDA,
            SNDRV_HWDEP_IFACE_USB_STREAM,
        ];
        for i in 0..ifaces.len() {
            for j in (i + 1)..ifaces.len() {
                assert_ne!(ifaces[i], ifaces[j]);
            }
        }
    }

    #[test]
    fn test_open_flags_no_overlap() {
        let flags = [
            SNDRV_HWDEP_OPEN_READ,
            SNDRV_HWDEP_OPEN_WRITE,
            SNDRV_HWDEP_OPEN_NONBLOCK,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
