//! `<linux/miscdevice.h>` — Miscellaneous device framework constants.
//!
//! The misc device framework provides a simplified interface for
//! character devices that need only a single minor number and basic
//! file operations. Instead of allocating a full major number, misc
//! devices share major 10 and get a dynamic or well-known minor.
//! Used by watchdog timers, hardware RNG, device-mapper, loop
//! devices, KVM, fuse, TUN/TAP, and hundreds of other small drivers.

// ---------------------------------------------------------------------------
// Well-known misc minor numbers
// ---------------------------------------------------------------------------

/// PSMOUSE (PS/2 mouse) minor.
pub const PSMOUSE_MINOR: u32 = 1;
/// Microsoft BusMouse minor.
pub const MS_BUSMOUSE_MINOR: u32 = 2;
/// ATIXL BusMouse minor.
pub const ATIXL_BUSMOUSE_MINOR: u32 = 3;
/// Watchdog timer minor.
pub const WATCHDOG_MINOR: u32 = 130;
/// Temperature sensor minor.
pub const TEMP_MINOR: u32 = 131;
/// Hardware RNG (hw_random) minor.
pub const HWRNG_MINOR: u32 = 183;
/// Microcode update minor.
pub const MICROCODE_MINOR: u32 = 184;
/// VGA arbiter minor.
pub const VGAARB_MINOR: u32 = 63;
/// Device mapper control minor.
pub const MAPPER_CTRL_MINOR: u32 = 236;
/// Loop control minor.
pub const LOOP_CTRL_MINOR: u32 = 237;
/// TUN/TAP minor.
pub const TUN_MINOR: u32 = 200;
/// FUSE minor.
pub const FUSE_MINOR: u32 = 229;
/// KVM minor.
pub const KVM_MINOR: u32 = 232;
/// snapshot (device-mapper) minor.
pub const SNAPSHOT_MINOR: u32 = 231;
/// Xen privcmd minor.
pub const XEN_MINOR: u32 = 203;
/// User-mode input minor.
pub const UINPUT_MINOR: u32 = 223;
/// hpet (High Precision Event Timer) minor.
pub const HPET_MINOR: u32 = 228;

/// Dynamic minor allocation (let the kernel choose).
pub const MISC_DYNAMIC_MINOR: u32 = 255;

// ---------------------------------------------------------------------------
// Misc device major number
// ---------------------------------------------------------------------------

/// Major number shared by all misc devices.
pub const MISC_MAJOR: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_well_known_minors_distinct() {
        let minors = [
            PSMOUSE_MINOR, MS_BUSMOUSE_MINOR, ATIXL_BUSMOUSE_MINOR,
            WATCHDOG_MINOR, TEMP_MINOR, HWRNG_MINOR,
            MICROCODE_MINOR, VGAARB_MINOR, MAPPER_CTRL_MINOR,
            LOOP_CTRL_MINOR, TUN_MINOR, FUSE_MINOR,
            KVM_MINOR, SNAPSHOT_MINOR, XEN_MINOR,
            UINPUT_MINOR, HPET_MINOR,
        ];
        for i in 0..minors.len() {
            for j in (i + 1)..minors.len() {
                assert_ne!(minors[i], minors[j]);
            }
        }
    }

    #[test]
    fn test_major_number() {
        assert_eq!(MISC_MAJOR, 10);
    }

    #[test]
    fn test_dynamic_minor() {
        assert_eq!(MISC_DYNAMIC_MINOR, 255);
    }

    #[test]
    fn test_minors_in_range() {
        // Minor numbers are 0-255 (8-bit field)
        let minors = [
            PSMOUSE_MINOR, MS_BUSMOUSE_MINOR, ATIXL_BUSMOUSE_MINOR,
            WATCHDOG_MINOR, TEMP_MINOR, HWRNG_MINOR,
            MICROCODE_MINOR, VGAARB_MINOR, MAPPER_CTRL_MINOR,
            LOOP_CTRL_MINOR, TUN_MINOR, FUSE_MINOR,
            KVM_MINOR, SNAPSHOT_MINOR, XEN_MINOR,
            UINPUT_MINOR, HPET_MINOR, MISC_DYNAMIC_MINOR,
        ];
        for m in minors {
            assert!(m <= 255);
        }
    }
}
