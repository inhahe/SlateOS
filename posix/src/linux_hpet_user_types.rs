//! `<linux/hpet.h>` — `/dev/hpet` periodic-timer userspace ioctls.
//!
//! On x86_64 systems the High-Precision Event Timer (HPET) is
//! exported as `/dev/hpetN`. Test rigs and userspace audio /
//! latency-measurement tools open it and arm a periodic interrupt
//! using the ioctls below.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for /dev/hpet ioctls ('h').
pub const HPET_IOC_MAGIC: u8 = b'h';

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `HPET_IE_ON` — enable interrupts on the timer.
pub const HPET_IE_ON: u32 = 0x0000_6801;
/// `HPET_IE_OFF` — disable interrupts on the timer.
pub const HPET_IE_OFF: u32 = 0x0000_6802;
/// `HPET_INFO` — query struct hpet_info.
pub const HPET_INFO: u32 = 0x8000_6803;
/// `HPET_EPI` — request edge-triggered periodic interrupts.
pub const HPET_EPI: u32 = 0x0000_6804;
/// `HPET_DPI` — request edge-triggered one-shot interrupts.
pub const HPET_DPI: u32 = 0x0000_6805;
/// `HPET_IRQFREQ` — set the periodic frequency (Hz).
pub const HPET_IRQFREQ: u32 = 0x4004_6806;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of HPET timers per chip.
pub const HPET_MAX_TIMERS: u32 = 32;
/// Default tick frequency (Hz) per the spec.
pub const HPET_TICK_FREQ: u32 = 14_318_180;
/// Maximum allowed frequency for HPET_IRQFREQ (8 KHz).
pub const HPET_USER_FREQ: u32 = 8192;

// ---------------------------------------------------------------------------
// hpet_info.hi_flags bits
// ---------------------------------------------------------------------------

/// Timer supports periodic mode.
pub const HPET_FLAG_PERIODIC: u32 = 1 << 0;
/// Timer supports edge-triggered interrupts.
pub const HPET_FLAG_EDGE: u32 = 1 << 1;
/// Timer supports level-triggered interrupts.
pub const HPET_FLAG_LEVEL: u32 = 1 << 2;
/// Timer supports 64-bit width.
pub const HPET_FLAG_64BIT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_h() {
        assert_eq!(HPET_IOC_MAGIC, b'h');
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_h() {
        let ops = [
            HPET_IE_ON, HPET_IE_OFF, HPET_INFO, HPET_EPI, HPET_DPI, HPET_IRQFREQ,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'h' (0x68) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'h' as u32);
        }
    }

    #[test]
    fn test_limits_sane() {
        // 32 timers/chip is the max addressable by the 5-bit timer id.
        assert_eq!(HPET_MAX_TIMERS, 32);
        // 14.318180 MHz is the historical NTSC-derived HPET tick.
        assert_eq!(HPET_TICK_FREQ, 14_318_180);
        // 8 KHz user-frequency cap matches the kernel sysctl default.
        assert_eq!(HPET_USER_FREQ, 8192);
        assert!(HPET_USER_FREQ.is_power_of_two());
    }

    #[test]
    fn test_flag_bits_pow2_distinct() {
        let f = [
            HPET_FLAG_PERIODIC,
            HPET_FLAG_EDGE,
            HPET_FLAG_LEVEL,
            HPET_FLAG_64BIT,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }
}
