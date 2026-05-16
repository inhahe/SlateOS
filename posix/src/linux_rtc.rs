//! `<linux/rtc.h>` — real-time clock (RTC) device interface.
//!
//! Provides ioctl constants and data structures for accessing the
//! hardware RTC via `/dev/rtc`.

// ---------------------------------------------------------------------------
// RTC time struct
// ---------------------------------------------------------------------------

/// RTC time representation (matches `struct rtc_time`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RtcTime {
    /// Seconds [0..59].
    pub tm_sec: i32,
    /// Minutes [0..59].
    pub tm_min: i32,
    /// Hours [0..23].
    pub tm_hour: i32,
    /// Day of month [1..31].
    pub tm_mday: i32,
    /// Month [0..11].
    pub tm_mon: i32,
    /// Year since 1900.
    pub tm_year: i32,
    /// Day of week (unused by RTC, set by kernel).
    pub tm_wday: i32,
    /// Day of year (unused by RTC, set by kernel).
    pub tm_yday: i32,
    /// DST flag (unused by RTC, set by kernel).
    pub tm_isdst: i32,
}

/// RTC wake alarm.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtcWkalrm {
    /// 0 = alarm disabled, 1 = enabled.
    pub enabled: u8,
    /// 0 = alarm not pending, 1 = pending.
    pub pending: u8,
    /// Alarm time.
    pub time: RtcTime,
}

/// RTC PLL correction info.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtcPllInfo {
    /// PLL control value.
    pub pll_ctrl: i32,
    /// PLL value.
    pub pll_value: i32,
    /// PLL maximum positive adjustment.
    pub pll_max: i32,
    /// PLL minimum negative adjustment.
    pub pll_min: i32,
    /// PLL multiplicand.
    pub pll_posmult: i32,
    /// PLL divisor.
    pub pll_negmult: i32,
    /// PLL clock.
    pub pll_clock: i64,
}

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Read RTC time.
pub const RTC_RD_TIME: u64 = 0x8024_7009;
/// Set RTC time.
pub const RTC_SET_TIME: u64 = 0x4024_700A;
/// Read alarm.
pub const RTC_ALM_READ: u64 = 0x8024_7008;
/// Set alarm.
pub const RTC_ALM_SET: u64 = 0x4024_7007;
/// Enable alarm interrupt.
pub const RTC_AIE_ON: u64 = 0x7001;
/// Disable alarm interrupt.
pub const RTC_AIE_OFF: u64 = 0x7002;
/// Enable update interrupt.
pub const RTC_UIE_ON: u64 = 0x7003;
/// Disable update interrupt.
pub const RTC_UIE_OFF: u64 = 0x7004;
/// Enable periodic interrupt.
pub const RTC_PIE_ON: u64 = 0x7005;
/// Disable periodic interrupt.
pub const RTC_PIE_OFF: u64 = 0x7006;
/// Set periodic interrupt rate.
pub const RTC_IRQP_SET: u64 = 0x400C_700C;
/// Get periodic interrupt rate.
pub const RTC_IRQP_READ: u64 = 0x800C_700B;
/// Read wake alarm.
pub const RTC_WKALM_RD: u64 = 0x8028_7010;
/// Set wake alarm.
pub const RTC_WKALM_SET: u64 = 0x4028_700F;
/// Get RTC epoch.
pub const RTC_EPOCH_READ: u64 = 0x800C_700D;
/// Set RTC epoch.
pub const RTC_EPOCH_SET: u64 = 0x400C_700E;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtc_time_size() {
        // 9 i32 fields = 36 bytes.
        assert_eq!(core::mem::size_of::<RtcTime>(), 36);
    }

    #[test]
    fn test_rtc_wkalrm_struct() {
        let alarm = RtcWkalrm {
            enabled: 1,
            pending: 0,
            time: unsafe { core::mem::zeroed() },
        };
        assert_eq!(alarm.enabled, 1);
        assert_eq!(alarm.pending, 0);
    }

    #[test]
    fn test_rtc_pll_info_size() {
        assert!(core::mem::size_of::<RtcPllInfo>() >= 32);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            RTC_RD_TIME, RTC_SET_TIME, RTC_ALM_READ, RTC_ALM_SET,
            RTC_AIE_ON, RTC_AIE_OFF, RTC_UIE_ON, RTC_UIE_OFF,
            RTC_PIE_ON, RTC_PIE_OFF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rtc_time_values() {
        let t = RtcTime {
            tm_sec: 30,
            tm_min: 45,
            tm_hour: 14,
            tm_mday: 15,
            tm_mon: 4,  // May (0-indexed)
            tm_year: 126, // 2026
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
        };
        assert_eq!(t.tm_year + 1900, 2026);
        assert_eq!(t.tm_mon + 1, 5);
    }

    #[test]
    fn test_interrupt_toggle_pairs() {
        // ON/OFF pairs should be distinct but related.
        assert_ne!(RTC_AIE_ON, RTC_AIE_OFF);
        assert_ne!(RTC_UIE_ON, RTC_UIE_OFF);
        assert_ne!(RTC_PIE_ON, RTC_PIE_OFF);
    }
}
