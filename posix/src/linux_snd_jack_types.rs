//! `<sound/jack.h>` — ALSA jack detection event constants.
//!
//! Jack detection reports the physical connection state of audio
//! jacks (headphone, microphone, line in/out). The kernel generates
//! input events and ALSA kcontrol notifications when a plug is
//! inserted or removed, allowing userspace to reroute audio.

// ---------------------------------------------------------------------------
// Jack type bits (what kind of jack)
// ---------------------------------------------------------------------------

/// Headphone jack.
pub const SND_JACK_HEADPHONE: u32 = 1 << 0;
/// Microphone jack.
pub const SND_JACK_MICROPHONE: u32 = 1 << 1;
/// Headset (headphone + microphone combo).
pub const SND_JACK_HEADSET: u32 = (1 << 0) | (1 << 1);
/// Line out jack.
pub const SND_JACK_LINEOUT: u32 = 1 << 2;
/// Mechanical switch (lid, dock, etc.).
pub const SND_JACK_MECHANICAL: u32 = 1 << 3;
/// Video out jack (HDMI/DP audio).
pub const SND_JACK_VIDEOOUT: u32 = 1 << 4;
/// Line in jack.
pub const SND_JACK_LINEIN: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Jack button bits (inline buttons on headset)
// ---------------------------------------------------------------------------

/// Button 0 (play/pause, typically).
pub const SND_JACK_BTN_0: u32 = 0x4000;
/// Button 1 (volume up, typically).
pub const SND_JACK_BTN_1: u32 = 0x2000;
/// Button 2 (volume down, typically).
pub const SND_JACK_BTN_2: u32 = 0x1000;
/// Button 3 (assistant/voice, typically).
pub const SND_JACK_BTN_3: u32 = 0x0800;
/// Button 4.
pub const SND_JACK_BTN_4: u32 = 0x0400;
/// Button 5.
pub const SND_JACK_BTN_5: u32 = 0x0200;

// ---------------------------------------------------------------------------
// Jack detection status
// ---------------------------------------------------------------------------

/// Jack is unplugged.
pub const SND_JACK_STATUS_UNPLUGGED: u32 = 0;
/// Jack is plugged in (value is OR of type bits).
pub const SND_JACK_STATUS_PLUGGED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jack_type_bits() {
        let types = [
            SND_JACK_HEADPHONE,
            SND_JACK_MICROPHONE,
            SND_JACK_LINEOUT,
            SND_JACK_MECHANICAL,
            SND_JACK_VIDEOOUT,
            SND_JACK_LINEIN,
        ];
        for i in 0..types.len() {
            assert!(types[i].is_power_of_two());
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_headset_is_combo() {
        assert_eq!(SND_JACK_HEADSET, SND_JACK_HEADPHONE | SND_JACK_MICROPHONE);
    }

    #[test]
    fn test_button_bits_distinct() {
        let btns = [
            SND_JACK_BTN_0,
            SND_JACK_BTN_1,
            SND_JACK_BTN_2,
            SND_JACK_BTN_3,
            SND_JACK_BTN_4,
            SND_JACK_BTN_5,
        ];
        for i in 0..btns.len() {
            assert!(btns[i].is_power_of_two());
            for j in (i + 1)..btns.len() {
                assert_eq!(btns[i] & btns[j], 0);
            }
        }
    }

    #[test]
    fn test_buttons_dont_overlap_types() {
        let type_mask = SND_JACK_HEADPHONE
            | SND_JACK_MICROPHONE
            | SND_JACK_LINEOUT
            | SND_JACK_MECHANICAL
            | SND_JACK_VIDEOOUT
            | SND_JACK_LINEIN;
        let btn_mask = SND_JACK_BTN_0
            | SND_JACK_BTN_1
            | SND_JACK_BTN_2
            | SND_JACK_BTN_3
            | SND_JACK_BTN_4
            | SND_JACK_BTN_5;
        assert_eq!(type_mask & btn_mask, 0);
    }

    #[test]
    fn test_status_values() {
        assert_eq!(SND_JACK_STATUS_UNPLUGGED, 0);
        assert_ne!(SND_JACK_STATUS_UNPLUGGED, SND_JACK_STATUS_PLUGGED);
    }
}
