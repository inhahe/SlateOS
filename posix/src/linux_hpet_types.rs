//! `<linux/hpet.h>` — High Precision Event Timer userspace ioctls.
//!
//! Constants for the HPET character-device interface
//! (`/dev/hpet`). The kernel exposes per-timer file descriptors that
//! userland uses for sub-millisecond polling timers.

// ---------------------------------------------------------------------------
// HPET ioctl numbers
// ---------------------------------------------------------------------------
//
// The encoding follows `_IO`/`_IOR`/`_IOW`/`_IOWR` with type 'h' and
// the numbers below. We store the raw encoded values to avoid pulling
// in the full `_IOC` macro infrastructure at this layer.

/// HPET_IE_ON: start interrupts.
pub const HPET_IE_ON: u32 = 0x6801;
/// HPET_IE_OFF: stop interrupts.
pub const HPET_IE_OFF: u32 = 0x6802;
/// HPET_INFO: query timer info (returns struct hpet_info).
pub const HPET_INFO: u32 = 0x40206803;
/// HPET_EPI: enable periodic interrupt.
pub const HPET_EPI: u32 = 0x6804;
/// HPET_DPI: disable periodic interrupt.
pub const HPET_DPI: u32 = 0x6805;
/// HPET_IRQFREQ: set interrupt frequency (Hz).
pub const HPET_IRQFREQ: u32 = 0x40046806;

// ---------------------------------------------------------------------------
// hpet_info.hi_flags bits
// ---------------------------------------------------------------------------

/// Timer is periodic-capable.
pub const HPET_INFO_PERIODIC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Misc constants
// ---------------------------------------------------------------------------

/// Maximum number of timers supported per HPET block (Linux uapi cap).
pub const HPET_MAX_TIMERS: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            HPET_IE_ON,
            HPET_IE_OFF,
            HPET_INFO,
            HPET_EPI,
            HPET_DPI,
            HPET_IRQFREQ,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_periodic_flag_bit() {
        assert!(HPET_INFO_PERIODIC.is_power_of_two());
    }

    #[test]
    fn test_max_timers_sensible() {
        assert_eq!(HPET_MAX_TIMERS, 32);
        assert!(HPET_MAX_TIMERS.is_power_of_two());
    }
}
