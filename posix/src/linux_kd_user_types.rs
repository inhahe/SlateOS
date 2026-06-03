//! `<linux/kd.h>` — virtual-console KD ioctl ABI.
//!
//! Every tty utility (`kbdrate`, `setleds`, `loadkeys`, `setfont`,
//! `chvt`, `openvt`, `console-tools`) drives the kernel's VT layer
//! via the constants below. They predate evdev and have been
//! frozen since the 2.4 days, so the values can be relied on.

// ---------------------------------------------------------------------------
// ioctl numbers — `KD` magic 'K' = 0x4B
// ---------------------------------------------------------------------------

/// Get keyboard type.
pub const KDGKBTYPE: u32 = 0x4B33;
/// Sound generator on.
pub const KIOCSOUND: u32 = 0x4B2F;
/// Generate tone (ms in low 16, hz in high 16).
pub const KDMKTONE: u32 = 0x4B30;
/// Get LED flags.
pub const KDGETLED: u32 = 0x4B31;
/// Set LED flags.
pub const KDSETLED: u32 = 0x4B32;
/// Get keyboard mode.
pub const KDGKBMODE: u32 = 0x4B44;
/// Set keyboard mode.
pub const KDSKBMODE: u32 = 0x4B45;
/// Get text/graphics mode.
pub const KDGETMODE: u32 = 0x4B3B;
/// Set text/graphics mode.
pub const KDSETMODE: u32 = 0x4B3A;
/// Map the framebuffer (legacy).
pub const KDMAPDISP: u32 = 0x4B37;
/// Unmap the framebuffer.
pub const KDUNMAPDISP: u32 = 0x4B38;
/// Get keymap entry.
pub const KDGKBENT: u32 = 0x4B46;
/// Set keymap entry.
pub const KDSKBENT: u32 = 0x4B47;
/// Get accent table entry.
pub const KDGKBDIACR: u32 = 0x4B4A;
/// Set accent table entry.
pub const KDSKBDIACR: u32 = 0x4B4B;
/// Get keyboard meta mode.
pub const KDGKBMETA: u32 = 0x4B62;
/// Set keyboard meta mode.
pub const KDSKBMETA: u32 = 0x4B63;

// ---------------------------------------------------------------------------
// KDSETMODE values
// ---------------------------------------------------------------------------

pub const KD_TEXT: u32 = 0x00;
pub const KD_GRAPHICS: u32 = 0x01;
pub const KD_TEXT0: u32 = 0x02;
pub const KD_TEXT1: u32 = 0x03;

// ---------------------------------------------------------------------------
// KDSKBMODE values
// ---------------------------------------------------------------------------

pub const K_RAW: u32 = 0x00;
pub const K_XLATE: u32 = 0x01;
pub const K_MEDIUMRAW: u32 = 0x02;
pub const K_UNICODE: u32 = 0x03;
pub const K_OFF: u32 = 0x04;

// ---------------------------------------------------------------------------
// LED bitfield (KDSETLED / KDGETLED)
// ---------------------------------------------------------------------------

pub const LED_SCR: u8 = 0x01;
pub const LED_NUM: u8 = 0x02;
pub const LED_CAP: u8 = 0x04;

// ---------------------------------------------------------------------------
// KDSKBMETA values
// ---------------------------------------------------------------------------

pub const K_METABIT: u32 = 0x03;
pub const K_ESCPREFIX: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_magic_byte_is_K() {
        // All KD ioctls have the 'K' magic byte in bits 8..15.
        for c in [
            KDGKBTYPE,
            KIOCSOUND,
            KDMKTONE,
            KDGETLED,
            KDSETLED,
            KDGKBMODE,
            KDSKBMODE,
            KDGETMODE,
            KDSETMODE,
            KDMAPDISP,
            KDUNMAPDISP,
            KDGKBENT,
            KDSKBENT,
            KDGKBDIACR,
            KDSKBDIACR,
            KDGKBMETA,
            KDSKBMETA,
        ] {
            assert_eq!((c >> 8) & 0xFF, u32::from(b'K'));
        }
    }

    #[test]
    fn test_setmode_values_distinct() {
        let v = [KD_TEXT, KD_GRAPHICS, KD_TEXT0, KD_TEXT1];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_kbmode_values_dense_0_to_4() {
        let m = [K_RAW, K_XLATE, K_MEDIUMRAW, K_UNICODE, K_OFF];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_led_bits_single_bit() {
        for &b in &[LED_SCR, LED_NUM, LED_CAP] {
            assert!(b.is_power_of_two());
        }
        // OR of all three covers the low 3 bits.
        assert_eq!(LED_SCR | LED_NUM | LED_CAP, 0b0000_0111);
    }

    #[test]
    fn test_kbmeta_distinct() {
        assert_ne!(K_METABIT, K_ESCPREFIX);
        assert!(K_METABIT < K_ESCPREFIX);
    }

    #[test]
    fn test_known_specific_values() {
        // KDGKBTYPE = 0x4B33 — verified against kernel UAPI for decades.
        assert_eq!(KDGKBTYPE, 0x4B33);
        // KDSETLED = 0x4B32 — `setleds(1)` uses this.
        assert_eq!(KDSETLED, 0x4B32);
        // KIOCSOUND = 0x4B2F — `beep(1)` uses this.
        assert_eq!(KIOCSOUND, 0x4B2F);
    }
}
