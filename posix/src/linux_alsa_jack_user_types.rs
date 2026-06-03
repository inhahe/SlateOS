//! `<sound/jack.h>` — ALSA jack-detection types and reporting bits.
//!
//! ALSA reports physical-connector state (headphone plug, microphone
//! plug, HDMI hot-plug, etc.) through "jack" kcontrols and through
//! the input subsystem as `SW_*` switches. This module collects the
//! identifiers used at the user-space boundary.

// ---------------------------------------------------------------------------
// `snd_jack_types` — kind of physical connector
// ---------------------------------------------------------------------------

pub const SND_JACK_HEADPHONE: u32 = 1 << 0;
pub const SND_JACK_MICROPHONE: u32 = 1 << 1;
pub const SND_JACK_LINEOUT: u32 = 1 << 2;
pub const SND_JACK_MECHANICAL: u32 = 1 << 3;
pub const SND_JACK_VIDEOOUT: u32 = 1 << 4;
pub const SND_JACK_LINEIN: u32 = 1 << 5;

/// Convenience combined headset (headphone + microphone).
pub const SND_JACK_HEADSET: u32 = SND_JACK_HEADPHONE | SND_JACK_MICROPHONE;
/// Convenience HDMI/DP audio sink (video + headphone).
pub const SND_JACK_AVOUT: u32 = SND_JACK_LINEOUT | SND_JACK_VIDEOOUT;

// ---------------------------------------------------------------------------
// Button / inline-remote bits (above 0x7FFF, separate range from connectors)
// ---------------------------------------------------------------------------

pub const SND_JACK_BTN_0: u32 = 1 << 30;
pub const SND_JACK_BTN_1: u32 = 1 << 29;
pub const SND_JACK_BTN_2: u32 = 1 << 28;
pub const SND_JACK_BTN_3: u32 = 1 << 27;
pub const SND_JACK_BTN_4: u32 = 1 << 26;
pub const SND_JACK_BTN_5: u32 = 1 << 25;

/// Mask of all six button bits.
pub const SND_JACK_BTNS_ALL: u32 = SND_JACK_BTN_0
    | SND_JACK_BTN_1
    | SND_JACK_BTN_2
    | SND_JACK_BTN_3
    | SND_JACK_BTN_4
    | SND_JACK_BTN_5;

// ---------------------------------------------------------------------------
// Linux input-subsystem switch codes mirroring jack state (`SW_*`)
// ---------------------------------------------------------------------------

pub const SW_HEADPHONE_INSERT: u16 = 0x02;
pub const SW_MICROPHONE_INSERT: u16 = 0x04;
pub const SW_LINEOUT_INSERT: u16 = 0x06;
pub const SW_JACK_PHYSICAL_INSERT: u16 = 0x07;
pub const SW_VIDEOOUT_INSERT: u16 = 0x0B;
pub const SW_LINEIN_INSERT: u16 = 0x0D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_bits_low_six_disjoint() {
        let c = [
            SND_JACK_HEADPHONE,
            SND_JACK_MICROPHONE,
            SND_JACK_LINEOUT,
            SND_JACK_MECHANICAL,
            SND_JACK_VIDEOOUT,
            SND_JACK_LINEIN,
        ];
        let mut or = 0u32;
        for v in c {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x3F);
    }

    #[test]
    fn test_headset_and_avout_composites() {
        assert_eq!(
            SND_JACK_HEADSET,
            SND_JACK_HEADPHONE | SND_JACK_MICROPHONE
        );
        assert_eq!(SND_JACK_AVOUT, SND_JACK_LINEOUT | SND_JACK_VIDEOOUT);
        // Composites are NOT power-of-two — they share bits.
        assert!(!SND_JACK_HEADSET.is_power_of_two());
    }

    #[test]
    fn test_button_bits_high_range_and_disjoint_from_connectors() {
        let b = [
            SND_JACK_BTN_0,
            SND_JACK_BTN_1,
            SND_JACK_BTN_2,
            SND_JACK_BTN_3,
            SND_JACK_BTN_4,
            SND_JACK_BTN_5,
        ];
        for v in b {
            assert!(v.is_power_of_two());
            // Connector bits live in 0..6; buttons in 25..31.
            assert!(v >= 1 << 25);
        }
        // BTNS_ALL is the OR of all six.
        assert_eq!(SND_JACK_BTNS_ALL.count_ones(), 6);
        // No overlap with connector mask.
        assert_eq!(SND_JACK_BTNS_ALL & 0x3F, 0);
    }

    #[test]
    fn test_button_ids_decreasing_bit_position() {
        // BTN_0 is the topmost (bit 30), BTN_5 the bottommost of the six.
        assert!(SND_JACK_BTN_0 > SND_JACK_BTN_1);
        assert!(SND_JACK_BTN_1 > SND_JACK_BTN_2);
        assert!(SND_JACK_BTN_2 > SND_JACK_BTN_3);
        assert!(SND_JACK_BTN_3 > SND_JACK_BTN_4);
        assert!(SND_JACK_BTN_4 > SND_JACK_BTN_5);
    }

    #[test]
    fn test_sw_codes_distinct_and_below_0x10() {
        let s = [
            SW_HEADPHONE_INSERT,
            SW_MICROPHONE_INSERT,
            SW_LINEOUT_INSERT,
            SW_JACK_PHYSICAL_INSERT,
            SW_VIDEOOUT_INSERT,
            SW_LINEIN_INSERT,
        ];
        for &v in &s {
            assert!(v < 0x10);
        }
        // Pairwise distinct.
        for (i, &a) in s.iter().enumerate() {
            for &b in &s[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
