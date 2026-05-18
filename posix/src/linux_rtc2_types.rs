//! `<linux/rtc.h>` — Additional RTC (Real-Time Clock) constants.
//!
//! Supplementary RTC constants covering IOCTL commands,
//! alarm flags, and feature bits.

// ---------------------------------------------------------------------------
// RTC IOCTL commands
// ---------------------------------------------------------------------------

/// Read time.
pub const RTC_RD_TIME: u32 = 0x80247009;
/// Set time.
pub const RTC_SET_TIME: u32 = 0x4024700A;
/// Read alarm.
pub const RTC_ALM_READ: u32 = 0x80247008;
/// Set alarm.
pub const RTC_ALM_SET: u32 = 0x40247007;
/// Enable alarm interrupts.
pub const RTC_AIE_ON: u32 = 0x00007001;
/// Disable alarm interrupts.
pub const RTC_AIE_OFF: u32 = 0x00007002;
/// Enable update interrupts.
pub const RTC_UIE_ON: u32 = 0x00007003;
/// Disable update interrupts.
pub const RTC_UIE_OFF: u32 = 0x00007004;
/// Enable periodic interrupts.
pub const RTC_PIE_ON: u32 = 0x00007005;
/// Disable periodic interrupts.
pub const RTC_PIE_OFF: u32 = 0x00007006;
/// Set periodic interrupt rate.
pub const RTC_IRQP_SET: u32 = 0x4008700C;
/// Read periodic interrupt rate.
pub const RTC_IRQP_READ: u32 = 0x8008700B;
/// Read wakeup alarm.
pub const RTC_WKALM_RD: u32 = 0x80287010;
/// Set wakeup alarm.
pub const RTC_WKALM_SET: u32 = 0x4028700F;

// ---------------------------------------------------------------------------
// RTC alarm flags
// ---------------------------------------------------------------------------

/// Alarm enabled.
pub const RTC_AF: u32 = 0x20;
/// Update flag.
pub const RTC_UF: u32 = 0x10;
/// Periodic flag.
pub const RTC_PF: u32 = 0x40;
/// Interrupt requested.
pub const RTC_IRQF: u32 = 0x80;

// ---------------------------------------------------------------------------
// RTC features
// ---------------------------------------------------------------------------

/// Has alarm.
pub const RTC_FEATURE_ALARM: u32 = 1 << 0;
/// Has alarm wakeup.
pub const RTC_FEATURE_ALARM_RES_MINUTE: u32 = 1 << 1;
/// Needs week day.
pub const RTC_FEATURE_NEED_WEEK_DAY: u32 = 1 << 2;
/// Alarm relative.
pub const RTC_FEATURE_ALARM_RES_2S: u32 = 1 << 3;
/// Update interrupt.
pub const RTC_FEATURE_UPDATE_INTERRUPT: u32 = 1 << 4;
/// Correction.
pub const RTC_FEATURE_CORRECTION: u32 = 1 << 5;
/// Backup switch mode.
pub const RTC_FEATURE_BACKUP_SWITCH_MODE: u32 = 1 << 6;
/// Alarm wakeup.
pub const RTC_FEATURE_ALARM_WAKEUP_ONLY: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            RTC_RD_TIME, RTC_SET_TIME, RTC_ALM_READ, RTC_ALM_SET,
            RTC_AIE_ON, RTC_AIE_OFF, RTC_UIE_ON, RTC_UIE_OFF,
            RTC_PIE_ON, RTC_PIE_OFF, RTC_IRQP_SET, RTC_IRQP_READ,
            RTC_WKALM_RD, RTC_WKALM_SET,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_alarm_flags_distinct() {
        let flags = [RTC_AF, RTC_UF, RTC_PF, RTC_IRQF];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_features_power_of_two() {
        let feats = [
            RTC_FEATURE_ALARM, RTC_FEATURE_ALARM_RES_MINUTE,
            RTC_FEATURE_NEED_WEEK_DAY, RTC_FEATURE_ALARM_RES_2S,
            RTC_FEATURE_UPDATE_INTERRUPT, RTC_FEATURE_CORRECTION,
            RTC_FEATURE_BACKUP_SWITCH_MODE, RTC_FEATURE_ALARM_WAKEUP_ONLY,
        ];
        for f in &feats {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }
}
