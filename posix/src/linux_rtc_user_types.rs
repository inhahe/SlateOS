//! `<linux/rtc.h>` — `/dev/rtc[N]` ioctl ABI.
//!
//! The RTC subsystem exposes a hardware real-time clock for use as
//! an alarm wakeup source (`RTC_WKALM_SET` is how systemd-rtcwake
//! suspends until a wall-clock time), a high-resolution timer
//! source, and the bootup `hwclock` --hctosys read. The ioctls
//! here use the `'p'` magic letter.

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

pub const DEV_RTC: &str = "/dev/rtc";
pub const DEV_RTC0: &str = "/dev/rtc0";

/// `'p'` — magic letter for RTC ioctls.
pub const RTC_IOC_MAGIC: u8 = b'p';

// ---------------------------------------------------------------------------
// Core RTC ioctls
// ---------------------------------------------------------------------------

pub const RTC_AIE_ON: u32 = 0x7001;
pub const RTC_AIE_OFF: u32 = 0x7002;
pub const RTC_UIE_ON: u32 = 0x7003;
pub const RTC_UIE_OFF: u32 = 0x7004;
pub const RTC_PIE_ON: u32 = 0x7005;
pub const RTC_PIE_OFF: u32 = 0x7006;
pub const RTC_WIE_ON: u32 = 0x700F;
pub const RTC_WIE_OFF: u32 = 0x7010;

pub const RTC_ALM_SET: u32 = 0x4024_7007;
pub const RTC_ALM_READ: u32 = 0x8024_7008;
pub const RTC_RD_TIME: u32 = 0x8024_7009;
pub const RTC_SET_TIME: u32 = 0x4024_700A;
pub const RTC_IRQP_READ: u32 = 0x8008_700B;
pub const RTC_IRQP_SET: u32 = 0x4008_700C;
pub const RTC_EPOCH_READ: u32 = 0x8008_700D;
pub const RTC_EPOCH_SET: u32 = 0x4008_700E;

pub const RTC_WKALM_SET: u32 = 0x4028_700F;
pub const RTC_WKALM_RD: u32 = 0x8028_7010;

// ---------------------------------------------------------------------------
// Interrupt status bits returned by `read(2)` on the RTC fd
// ---------------------------------------------------------------------------

/// Periodic interrupt occurred.
pub const RTC_PF: u32 = 0x40;
/// Alarm interrupt occurred.
pub const RTC_AF: u32 = 0x20;
/// Update interrupt occurred (1Hz).
pub const RTC_UF: u32 = 0x10;
/// All three interrupt-occurred bits.
pub const RTC_IRQF: u32 = 0x80;

// ---------------------------------------------------------------------------
// Periodic-interrupt frequency bounds
// ---------------------------------------------------------------------------

pub const RTC_MAX_FREQ: u32 = 8192;
/// Default frequency the RTC starts at on open.
pub const RTC_DEFAULT_FREQ: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_paths() {
        assert_eq!(DEV_RTC, "/dev/rtc");
        assert_eq!(DEV_RTC0, "/dev/rtc0");
        assert!(DEV_RTC0.starts_with(DEV_RTC));
    }

    #[test]
    fn test_ioc_magic_is_p() {
        assert_eq!(RTC_IOC_MAGIC, b'p');
        assert_eq!(RTC_IOC_MAGIC, 0x70);
    }

    #[test]
    fn test_aie_uie_pie_dense_pairs() {
        // Alarm/Update/Periodic interrupt-enable ioctls come in
        // ON/OFF pairs at consecutive numbers.
        assert_eq!(RTC_AIE_OFF, RTC_AIE_ON + 1);
        assert_eq!(RTC_UIE_OFF, RTC_UIE_ON + 1);
        assert_eq!(RTC_PIE_OFF, RTC_PIE_ON + 1);
    }

    #[test]
    fn test_periodic_freq_bounds() {
        // Periodic interrupt frequency is a power of two up to 8192 Hz.
        assert_eq!(RTC_MAX_FREQ, 8192);
        assert!(RTC_MAX_FREQ.is_power_of_two());
        assert_eq!(RTC_DEFAULT_FREQ, 1024);
        assert!(RTC_DEFAULT_FREQ.is_power_of_two());
        assert!(RTC_DEFAULT_FREQ < RTC_MAX_FREQ);
    }

    #[test]
    fn test_irq_status_bits_distinct_single_bit() {
        let s = [RTC_PF, RTC_AF, RTC_UF, RTC_IRQF];
        for v in s {
            assert!(v.is_power_of_two());
        }
        // RTC_IRQF is the "any" summary bit; it's distinct from the others.
        assert_eq!(RTC_IRQF, 0x80);
        assert_eq!(RTC_PF | RTC_AF | RTC_UF, 0x70);
    }

    #[test]
    fn test_data_ioctls_have_p_magic_byte() {
        // The mid-byte (bits 8..15) of the ioctl number carries the
        // magic letter for ioctls built via `_IOR`/`_IOW`.
        let i = [
            RTC_ALM_SET,
            RTC_ALM_READ,
            RTC_RD_TIME,
            RTC_SET_TIME,
            RTC_IRQP_READ,
            RTC_IRQP_SET,
            RTC_EPOCH_READ,
            RTC_EPOCH_SET,
            RTC_WKALM_SET,
            RTC_WKALM_RD,
        ];
        for &v in i.iter() {
            assert_eq!((v >> 8) & 0xFF, RTC_IOC_MAGIC as u32);
        }
    }
}
