//! `<sound/hwdep.h>` — ALSA hardware-dependent interface IDs and DSP states.
//!
//! HWDEP exposes vendor- or chip-specific interfaces (FPGA loading,
//! firmware upload, OPL synthesisers, etc.) when the standard PCM /
//! mixer / midi APIs aren't enough. Each implementation declares
//! which `iface` it speaks.

// ---------------------------------------------------------------------------
// `snd_hwdep_iface_t` — interface identifiers
// ---------------------------------------------------------------------------

pub const SNDRV_HWDEP_IFACE_OPL2: u32 = 0;
pub const SNDRV_HWDEP_IFACE_OPL3: u32 = 1;
pub const SNDRV_HWDEP_IFACE_OPL4: u32 = 2;
pub const SNDRV_HWDEP_IFACE_SB16CSP: u32 = 3;
pub const SNDRV_HWDEP_IFACE_EMU10K1: u32 = 4;
pub const SNDRV_HWDEP_IFACE_YSS225: u32 = 5;
pub const SNDRV_HWDEP_IFACE_ICS2115: u32 = 6;
pub const SNDRV_HWDEP_IFACE_SSCAPE: u32 = 7;
pub const SNDRV_HWDEP_IFACE_VX: u32 = 8;
pub const SNDRV_HWDEP_IFACE_MIXART: u32 = 9;
pub const SNDRV_HWDEP_IFACE_USX2Y: u32 = 10;
pub const SNDRV_HWDEP_IFACE_EMUX_WAVETABLE: u32 = 11;
pub const SNDRV_HWDEP_IFACE_BLUETOOTH: u32 = 12;
pub const SNDRV_HWDEP_IFACE_USX2Y_PCM: u32 = 13;
pub const SNDRV_HWDEP_IFACE_PCXHR: u32 = 14;
pub const SNDRV_HWDEP_IFACE_SB_RC: u32 = 15;
pub const SNDRV_HWDEP_IFACE_HDA: u32 = 16;
pub const SNDRV_HWDEP_IFACE_USB_STREAM: u32 = 17;
pub const SNDRV_HWDEP_IFACE_FW_DICE: u32 = 18;
pub const SNDRV_HWDEP_IFACE_FW_FIREWORKS: u32 = 19;
pub const SNDRV_HWDEP_IFACE_FW_BEBOB: u32 = 20;
pub const SNDRV_HWDEP_IFACE_FW_OXFW: u32 = 21;
pub const SNDRV_HWDEP_IFACE_FW_DIGI00X: u32 = 22;
pub const SNDRV_HWDEP_IFACE_FW_TASCAM: u32 = 23;
pub const SNDRV_HWDEP_IFACE_LINE6: u32 = 24;
pub const SNDRV_HWDEP_IFACE_FW_MOTU: u32 = 25;
pub const SNDRV_HWDEP_IFACE_FW_FIREFACE: u32 = 26;

/// Highest known iface ID — userspace sanity-check upper bound.
pub const SNDRV_HWDEP_IFACE_LAST: u32 = SNDRV_HWDEP_IFACE_FW_FIREFACE;

// ---------------------------------------------------------------------------
// DSP firmware-upload control bits (`SNDRV_HWDEP_DSP_LOAD_F_*`)
// ---------------------------------------------------------------------------

/// Loaded DSP image is required for normal operation.
pub const SNDRV_HWDEP_DSP_LOAD_F_FW_REQUIRED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// DSP image-name length
// ---------------------------------------------------------------------------

pub const SNDRV_HWDEP_DSP_IMAGE_NAME_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opl_family_dense_0_to_2() {
        assert_eq!(SNDRV_HWDEP_IFACE_OPL2, 0);
        assert_eq!(SNDRV_HWDEP_IFACE_OPL3, 1);
        assert_eq!(SNDRV_HWDEP_IFACE_OPL4, 2);
    }

    #[test]
    fn test_iface_ids_distinct_and_in_range() {
        let all = [
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
            SNDRV_HWDEP_IFACE_EMUX_WAVETABLE,
            SNDRV_HWDEP_IFACE_BLUETOOTH,
            SNDRV_HWDEP_IFACE_USX2Y_PCM,
            SNDRV_HWDEP_IFACE_PCXHR,
            SNDRV_HWDEP_IFACE_SB_RC,
            SNDRV_HWDEP_IFACE_HDA,
            SNDRV_HWDEP_IFACE_USB_STREAM,
            SNDRV_HWDEP_IFACE_FW_DICE,
            SNDRV_HWDEP_IFACE_FW_FIREWORKS,
            SNDRV_HWDEP_IFACE_FW_BEBOB,
            SNDRV_HWDEP_IFACE_FW_OXFW,
            SNDRV_HWDEP_IFACE_FW_DIGI00X,
            SNDRV_HWDEP_IFACE_FW_TASCAM,
            SNDRV_HWDEP_IFACE_LINE6,
            SNDRV_HWDEP_IFACE_FW_MOTU,
            SNDRV_HWDEP_IFACE_FW_FIREFACE,
        ];
        // Dense 0..26 — strictly monotonic.
        for w in all.windows(2) {
            assert_eq!(w[1], w[0] + 1);
        }
        assert_eq!(SNDRV_HWDEP_IFACE_LAST, *all.last().unwrap());
        assert_eq!(SNDRV_HWDEP_IFACE_LAST, 26);
    }

    #[test]
    fn test_fw_family_grouped() {
        // Firewire interfaces are clustered together (18..=23, 25..=26).
        assert!(SNDRV_HWDEP_IFACE_FW_DICE >= 18);
        assert!(SNDRV_HWDEP_IFACE_FW_FIREFACE >= 26);
        // HDA is the most common modern iface and predates the FW cluster.
        assert!(SNDRV_HWDEP_IFACE_HDA < SNDRV_HWDEP_IFACE_FW_DICE);
    }

    #[test]
    fn test_dsp_load_flag_is_single_bit() {
        assert!(SNDRV_HWDEP_DSP_LOAD_F_FW_REQUIRED.is_power_of_two());
    }

    #[test]
    fn test_image_name_len_64() {
        assert_eq!(SNDRV_HWDEP_DSP_IMAGE_NAME_LEN, 64);
    }
}
