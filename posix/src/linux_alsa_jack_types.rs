//! `<sound/jack.h>` — ALSA jack detection constants.
//!
//! Jack detection reports the physical connection state of audio
//! connectors: headphone plug insertion/removal, microphone detection,
//! line-in/out status, and HDMI/DisplayPort audio connection. The
//! kernel notifies userspace via input events or ALSA kcontrols,
//! allowing automatic audio routing (e.g., switch from speakers to
//! headphones when plugged in).

// ---------------------------------------------------------------------------
// Jack types
// ---------------------------------------------------------------------------

/// Headphone jack (output).
pub const SND_JACK_HEADPHONE: u32 = 0x0001;
/// Microphone jack (input).
pub const SND_JACK_MICROPHONE: u32 = 0x0002;
/// Headset (headphone + microphone combo).
pub const SND_JACK_HEADSET: u32 = 0x0003;
/// Line-out jack.
pub const SND_JACK_LINEOUT: u32 = 0x0004;
/// Line-in jack.
pub const SND_JACK_LINEIN: u32 = 0x0008;
/// Mechanical switch (lid, dock, etc.).
pub const SND_JACK_MECHANICAL: u32 = 0x0010;
/// Video out (HDMI/DP audio associated with video).
pub const SND_JACK_VIDEOOUT: u32 = 0x0020;
/// Optical (S/PDIF TOSLINK).
pub const SND_JACK_OPTICAL: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Jack button events (for headset buttons)
// ---------------------------------------------------------------------------

/// Button 0 (play/pause, hook switch).
pub const SND_JACK_BTN_0: u32 = 0x4000;
/// Button 1 (volume up).
pub const SND_JACK_BTN_1: u32 = 0x2000;
/// Button 2 (volume down).
pub const SND_JACK_BTN_2: u32 = 0x1000;
/// Button 3 (voice assistant).
pub const SND_JACK_BTN_3: u32 = 0x0800;
/// Button 4.
pub const SND_JACK_BTN_4: u32 = 0x0400;
/// Button 5.
pub const SND_JACK_BTN_5: u32 = 0x0200;

// ---------------------------------------------------------------------------
// Jack detection states
// ---------------------------------------------------------------------------

/// Jack is disconnected (nothing plugged in).
pub const SND_JACK_STATE_UNPLUGGED: u32 = 0;
/// Jack is connected.
pub const SND_JACK_STATE_PLUGGED: u32 = 1;

// ---------------------------------------------------------------------------
// Jack detection methods
// ---------------------------------------------------------------------------

/// GPIO-based detection.
pub const SND_JACK_DETECT_GPIO: u32 = 0;
/// Codec-internal jack detection (impedance sensing).
pub const SND_JACK_DETECT_CODEC: u32 = 1;
/// External IC detection (dedicated jack detect chip).
pub const SND_JACK_DETECT_EXTERNAL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jack_types_no_headset_overlap() {
        // Headset is combination of headphone + microphone
        assert_eq!(SND_JACK_HEADSET, SND_JACK_HEADPHONE | SND_JACK_MICROPHONE);
    }

    #[test]
    fn test_button_bits_no_overlap() {
        let buttons = [
            SND_JACK_BTN_0,
            SND_JACK_BTN_1,
            SND_JACK_BTN_2,
            SND_JACK_BTN_3,
            SND_JACK_BTN_4,
            SND_JACK_BTN_5,
        ];
        for i in 0..buttons.len() {
            assert!(buttons[i].is_power_of_two());
            for j in (i + 1)..buttons.len() {
                assert_eq!(buttons[i] & buttons[j], 0);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(SND_JACK_STATE_UNPLUGGED, SND_JACK_STATE_PLUGGED);
    }

    #[test]
    fn test_detect_methods_distinct() {
        let methods = [
            SND_JACK_DETECT_GPIO,
            SND_JACK_DETECT_CODEC,
            SND_JACK_DETECT_EXTERNAL,
        ];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }
}
