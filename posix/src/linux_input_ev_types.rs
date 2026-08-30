//! `<linux/input-event-codes.h>` (EV subset) — input event type codes.
//!
//! Every input event carries a type code that categorises what kind of
//! state change occurred. `EV_KEY` for key presses, `EV_REL` for
//! mouse movement, `EV_ABS` for touchscreen coordinates, etc. The
//! type determines which code namespace applies (KEY_*, REL_*, ABS_*).
//! `EV_SYN` is special: it bundles multiple simultaneous axis changes
//! into atomic input frames.

// ---------------------------------------------------------------------------
// Event type codes
// ---------------------------------------------------------------------------

/// Synchronisation event (frame boundary).
pub const EV_SYN: u16 = 0x00;
/// Key/button press or release.
pub const EV_KEY: u16 = 0x01;
/// Relative axis change (mouse movement).
pub const EV_REL: u16 = 0x02;
/// Absolute axis change (touchscreen, joystick).
pub const EV_ABS: u16 = 0x03;
/// Miscellaneous event.
pub const EV_MSC: u16 = 0x04;
/// Switch event (lid close, headphone plug).
pub const EV_SW: u16 = 0x05;
/// LED control event.
pub const EV_LED: u16 = 0x11;
/// Sound output event (beep).
pub const EV_SND: u16 = 0x12;
/// Auto-repeat event.
pub const EV_REP: u16 = 0x14;
/// Force feedback effect event.
pub const EV_FF: u16 = 0x15;
/// Power management event.
pub const EV_PWR: u16 = 0x16;
/// Force feedback status event.
pub const EV_FF_STATUS: u16 = 0x17;

// ---------------------------------------------------------------------------
// Synchronisation sub-codes
// ---------------------------------------------------------------------------

/// Report synchronisation (end of event frame).
pub const SYN_REPORT: u16 = 0;
/// Configuration synchronisation.
pub const SYN_CONFIG: u16 = 1;
/// Multi-touch slot synchronisation (type-B protocol).
pub const SYN_MT_REPORT: u16 = 2;
/// Device dropped events (buffer overrun).
pub const SYN_DROPPED: u16 = 3;

// ---------------------------------------------------------------------------
// Miscellaneous event codes (EV_MSC)
// ---------------------------------------------------------------------------

/// Raw scancode.
pub const MSC_SERIAL: u16 = 0x00;
/// Pulse-per-second signal.
pub const MSC_PULSELED: u16 = 0x01;
/// Gesture code.
pub const MSC_GESTURE: u16 = 0x02;
/// Raw scancode from device.
pub const MSC_RAW: u16 = 0x03;
/// Scancode (HID usage → Linux keycode before mapping).
pub const MSC_SCAN: u16 = 0x04;
/// Timestamp.
pub const MSC_TIMESTAMP: u16 = 0x05;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum event type code.
pub const EV_MAX: u16 = 0x1F;
/// Number of event type codes.
pub const EV_CNT: u16 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ev_types_distinct() {
        let types = [
            EV_SYN,
            EV_KEY,
            EV_REL,
            EV_ABS,
            EV_MSC,
            EV_SW,
            EV_LED,
            EV_SND,
            EV_REP,
            EV_FF,
            EV_PWR,
            EV_FF_STATUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "EV types {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_ev_syn_is_zero() {
        assert_eq!(EV_SYN, 0);
    }

    #[test]
    fn test_core_ev_types_ordered() {
        assert!(EV_SYN < EV_KEY);
        assert!(EV_KEY < EV_REL);
        assert!(EV_REL < EV_ABS);
        assert!(EV_ABS < EV_MSC);
        assert!(EV_MSC < EV_SW);
    }

    #[test]
    fn test_syn_codes_distinct() {
        let syns = [SYN_REPORT, SYN_CONFIG, SYN_MT_REPORT, SYN_DROPPED];
        for i in 0..syns.len() {
            for j in (i + 1)..syns.len() {
                assert_ne!(syns[i], syns[j]);
            }
        }
    }

    #[test]
    fn test_msc_codes_distinct() {
        let mscs = [
            MSC_SERIAL,
            MSC_PULSELED,
            MSC_GESTURE,
            MSC_RAW,
            MSC_SCAN,
            MSC_TIMESTAMP,
        ];
        for i in 0..mscs.len() {
            for j in (i + 1)..mscs.len() {
                assert_ne!(mscs[i], mscs[j]);
            }
        }
    }

    #[test]
    fn test_all_within_max() {
        let types = [
            EV_SYN,
            EV_KEY,
            EV_REL,
            EV_ABS,
            EV_MSC,
            EV_SW,
            EV_LED,
            EV_SND,
            EV_REP,
            EV_FF,
            EV_PWR,
            EV_FF_STATUS,
        ];
        for &t in &types {
            assert!(t <= EV_MAX, "EV type 0x{:02X} exceeds EV_MAX", t);
        }
    }

    #[test]
    fn test_ev_cnt() {
        assert_eq!(EV_CNT, EV_MAX + 1);
    }
}
