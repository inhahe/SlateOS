//! `<linux/input-event-codes.h>` (SW subset) — switch event codes.
//!
//! Switch events report binary state changes for physical switches
//! that stay in one position until explicitly toggled. Examples
//! include laptop lid open/close, headphone jack insertion, and
//! tablet mode detection. Unlike key events, switches report
//! current state (0 or 1), not transitions.

// ---------------------------------------------------------------------------
// Switch codes
// ---------------------------------------------------------------------------

/// Lid is closed (laptop).
pub const SW_LID: u16 = 0x00;
/// Tablet mode is active (convertible laptop).
pub const SW_TABLET_MODE: u16 = 0x01;
/// Headphone jack is inserted.
pub const SW_HEADPHONE_INSERT: u16 = 0x02;
/// RF kill switch is active (radio off).
pub const SW_RFKILL_ALL: u16 = 0x03;
/// Microphone jack is inserted.
pub const SW_MICROPHONE_INSERT: u16 = 0x04;
/// Dock is connected.
pub const SW_DOCK: u16 = 0x05;
/// Line-out jack is inserted.
pub const SW_LINEOUT_INSERT: u16 = 0x06;
/// Jack physical insertion detection.
pub const SW_JACK_PHYSICAL_INSERT: u16 = 0x07;
/// Video output is inserted.
pub const SW_VIDEOOUT_INSERT: u16 = 0x08;
/// Camera lens cover is closed.
pub const SW_CAMERA_LENS_COVER: u16 = 0x09;
/// Keypad slide is open (slider phone).
pub const SW_KEYPAD_SLIDE: u16 = 0x0A;
/// Front proximity sensor is active.
pub const SW_FRONT_PROXIMITY: u16 = 0x0B;
/// External rotation lock is active.
pub const SW_ROTATE_LOCK: u16 = 0x0C;
/// Line-in jack is inserted.
pub const SW_LINEIN_INSERT: u16 = 0x0D;
/// Mute device (hardware mute switch).
pub const SW_MUTE_DEVICE: u16 = 0x0E;
/// Pen is inserted in its dock.
pub const SW_PEN_INSERTED: u16 = 0x0F;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum switch code.
pub const SW_MAX: u16 = 0x0F;
/// Number of switch codes (SW_MAX + 1).
pub const SW_CNT: u16 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sw_codes_distinct() {
        let sws = [
            SW_LID, SW_TABLET_MODE, SW_HEADPHONE_INSERT,
            SW_RFKILL_ALL, SW_MICROPHONE_INSERT, SW_DOCK,
            SW_LINEOUT_INSERT, SW_JACK_PHYSICAL_INSERT,
            SW_VIDEOOUT_INSERT, SW_CAMERA_LENS_COVER,
            SW_KEYPAD_SLIDE, SW_FRONT_PROXIMITY,
            SW_ROTATE_LOCK, SW_LINEIN_INSERT,
            SW_MUTE_DEVICE, SW_PEN_INSERTED,
        ];
        for i in 0..sws.len() {
            for j in (i + 1)..sws.len() {
                assert_ne!(sws[i], sws[j],
                    "switch codes {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_sw_sequential() {
        assert_eq!(SW_LID, 0);
        assert_eq!(SW_TABLET_MODE, 1);
        assert_eq!(SW_HEADPHONE_INSERT, 2);
        assert_eq!(SW_RFKILL_ALL, 3);
        assert_eq!(SW_MICROPHONE_INSERT, 4);
        assert_eq!(SW_DOCK, 5);
    }

    #[test]
    fn test_all_within_max() {
        let sws = [
            SW_LID, SW_TABLET_MODE, SW_HEADPHONE_INSERT,
            SW_RFKILL_ALL, SW_MICROPHONE_INSERT, SW_DOCK,
            SW_LINEOUT_INSERT, SW_JACK_PHYSICAL_INSERT,
            SW_VIDEOOUT_INSERT, SW_CAMERA_LENS_COVER,
            SW_KEYPAD_SLIDE, SW_FRONT_PROXIMITY,
            SW_ROTATE_LOCK, SW_LINEIN_INSERT,
            SW_MUTE_DEVICE, SW_PEN_INSERTED,
        ];
        for &s in &sws {
            assert!(s <= SW_MAX, "SW code 0x{:02X} exceeds SW_MAX", s);
        }
    }

    #[test]
    fn test_sw_cnt() {
        assert_eq!(SW_CNT, SW_MAX + 1);
    }
}
