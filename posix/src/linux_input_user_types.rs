//! `<linux/input.h>` — top-level evdev event types and IDs.
//!
//! The evdev character device (`/dev/input/event*`) is the canonical
//! Linux input ABI: every keyboard, mouse, joystick, touchscreen, and
//! pen tablet emits `struct input_event` records carrying a `type`
//! field from `EV_*` and a `code` field interpreted per type. X.Org,
//! Wayland compositors, libinput, SDL, and the kernel's `evtest`
//! consume these.

// ---------------------------------------------------------------------------
// EV_* — top-level event types (struct input_event.type)
// ---------------------------------------------------------------------------

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const EV_MSC: u16 = 0x04;
pub const EV_SW: u16 = 0x05;
pub const EV_LED: u16 = 0x11;
pub const EV_SND: u16 = 0x12;
pub const EV_REP: u16 = 0x14;
pub const EV_FF: u16 = 0x15;
pub const EV_PWR: u16 = 0x16;
pub const EV_FF_STATUS: u16 = 0x17;
/// One past the highest EV_* type.
pub const EV_MAX: u16 = 0x1F;
/// EV_* bitmask array length.
pub const EV_CNT: u32 = (EV_MAX as u32) + 1;

// ---------------------------------------------------------------------------
// SYN_* — codes for EV_SYN
// ---------------------------------------------------------------------------

pub const SYN_REPORT: u16 = 0;
pub const SYN_CONFIG: u16 = 1;
pub const SYN_MT_REPORT: u16 = 2;
pub const SYN_DROPPED: u16 = 3;

// ---------------------------------------------------------------------------
// MSC_* — codes for EV_MSC
// ---------------------------------------------------------------------------

pub const MSC_SERIAL: u16 = 0;
pub const MSC_PULSELED: u16 = 1;
pub const MSC_GESTURE: u16 = 2;
pub const MSC_RAW: u16 = 3;
pub const MSC_SCAN: u16 = 4;
pub const MSC_TIMESTAMP: u16 = 5;
pub const MSC_MAX: u16 = 7;

// ---------------------------------------------------------------------------
// Property flags (EVIOCGPROP)
// ---------------------------------------------------------------------------

pub const INPUT_PROP_POINTER: u16 = 0x00;
pub const INPUT_PROP_DIRECT: u16 = 0x01;
pub const INPUT_PROP_BUTTONPAD: u16 = 0x02;
pub const INPUT_PROP_SEMI_MT: u16 = 0x03;
pub const INPUT_PROP_TOPBUTTONPAD: u16 = 0x04;
pub const INPUT_PROP_POINTING_STICK: u16 = 0x05;
pub const INPUT_PROP_ACCELEROMETER: u16 = 0x06;
pub const INPUT_PROP_MAX: u16 = 0x1F;

// ---------------------------------------------------------------------------
// Bus types (struct input_id.bustype)
// ---------------------------------------------------------------------------

pub const BUS_PCI: u16 = 0x01;
pub const BUS_ISAPNP: u16 = 0x02;
pub const BUS_USB: u16 = 0x03;
pub const BUS_HIL: u16 = 0x04;
pub const BUS_BLUETOOTH: u16 = 0x05;
pub const BUS_VIRTUAL: u16 = 0x06;
pub const BUS_ISA: u16 = 0x10;
pub const BUS_I8042: u16 = 0x11;
pub const BUS_XTKBD: u16 = 0x12;
pub const BUS_RS232: u16 = 0x13;
pub const BUS_GAMEPORT: u16 = 0x14;
pub const BUS_PARPORT: u16 = 0x15;
pub const BUS_AMIGA: u16 = 0x16;
pub const BUS_ADB: u16 = 0x17;
pub const BUS_I2C: u16 = 0x18;
pub const BUS_HOST: u16 = 0x19;
pub const BUS_GSC: u16 = 0x1A;
pub const BUS_ATARI: u16 = 0x1B;
pub const BUS_SPI: u16 = 0x1C;
pub const BUS_RMI: u16 = 0x1D;
pub const BUS_CEC: u16 = 0x1E;
pub const BUS_INTEL_ISHTP: u16 = 0x1F;

// ---------------------------------------------------------------------------
// Key-repeat parameters (EV_REP codes)
// ---------------------------------------------------------------------------

pub const REP_DELAY: u16 = 0;
pub const REP_PERIOD: u16 = 1;
pub const REP_MAX: u16 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ev_low_types_dense_0_to_5() {
        // EV_SYN..EV_SW = 0..5.
        assert_eq!(EV_SYN, 0);
        assert_eq!(EV_KEY, 1);
        assert_eq!(EV_REL, 2);
        assert_eq!(EV_ABS, 3);
        assert_eq!(EV_MSC, 4);
        assert_eq!(EV_SW, 5);
    }

    #[test]
    fn test_ev_max_and_cnt() {
        assert_eq!(EV_MAX, 0x1F);
        assert_eq!(EV_CNT, 0x20);
        // Every defined type fits within EV_MAX.
        for &t in &[
            EV_SYN, EV_KEY, EV_REL, EV_ABS, EV_MSC, EV_SW, EV_LED, EV_SND, EV_REP, EV_FF, EV_PWR,
            EV_FF_STATUS,
        ] {
            assert!(t <= EV_MAX);
        }
    }

    #[test]
    fn test_syn_codes_dense() {
        let s = [SYN_REPORT, SYN_CONFIG, SYN_MT_REPORT, SYN_DROPPED];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_msc_codes_dense_0_to_5() {
        let m = [
            MSC_SERIAL,
            MSC_PULSELED,
            MSC_GESTURE,
            MSC_RAW,
            MSC_SCAN,
            MSC_TIMESTAMP,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert!(MSC_MAX >= MSC_TIMESTAMP);
    }

    #[test]
    fn test_input_prop_dense() {
        let p = [
            INPUT_PROP_POINTER,
            INPUT_PROP_DIRECT,
            INPUT_PROP_BUTTONPAD,
            INPUT_PROP_SEMI_MT,
            INPUT_PROP_TOPBUTTONPAD,
            INPUT_PROP_POINTING_STICK,
            INPUT_PROP_ACCELEROMETER,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert!(INPUT_PROP_MAX >= INPUT_PROP_ACCELEROMETER);
    }

    #[test]
    fn test_bus_types_distinct() {
        let b = [
            BUS_PCI,
            BUS_ISAPNP,
            BUS_USB,
            BUS_HIL,
            BUS_BLUETOOTH,
            BUS_VIRTUAL,
            BUS_ISA,
            BUS_I8042,
            BUS_XTKBD,
            BUS_RS232,
            BUS_GAMEPORT,
            BUS_PARPORT,
            BUS_AMIGA,
            BUS_ADB,
            BUS_I2C,
            BUS_HOST,
            BUS_GSC,
            BUS_ATARI,
            BUS_SPI,
            BUS_RMI,
            BUS_CEC,
            BUS_INTEL_ISHTP,
        ];
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
        // USB is the most common — must match the documented 0x03.
        assert_eq!(BUS_USB, 3);
    }

    #[test]
    fn test_rep_codes() {
        assert_eq!(REP_DELAY, 0);
        assert_eq!(REP_PERIOD, 1);
        assert_eq!(REP_MAX, REP_PERIOD);
    }
}
