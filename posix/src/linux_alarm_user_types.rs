//! `<linux/android_alarm.h>` and `<linux/rtc.h>` — wake-up alarms.
//!
//! Linux exposes wake-up-capable alarms through `timerfd_create` with
//! `CLOCK_BOOTTIME_ALARM` / `CLOCK_REALTIME_ALARM`, and through the
//! older `/dev/alarm` ioctls (Android). Both surfaces let user
//! programs wake the system from suspend at a scheduled time.

// ---------------------------------------------------------------------------
// Clock IDs that participate in wake-up
// ---------------------------------------------------------------------------

pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
pub const CLOCK_REALTIME_ALARM: u32 = 8;

// ---------------------------------------------------------------------------
// Android `/dev/alarm` ioctls (legacy)
// ---------------------------------------------------------------------------

pub const DEV_ALARM: &str = "/dev/alarm";

pub const ANDROID_ALARM_CLEAR_BASE: u32 = 0x4040_6106;
pub const ANDROID_ALARM_WAIT: u32 = 0x4040_6101;
pub const ANDROID_ALARM_SET_RTC: u32 = 0x4040_6105;

// ---------------------------------------------------------------------------
// Android alarm types
// ---------------------------------------------------------------------------

pub const ANDROID_ALARM_RTC_WAKEUP: u32 = 0;
pub const ANDROID_ALARM_RTC: u32 = 1;
pub const ANDROID_ALARM_ELAPSED_REALTIME_WAKEUP: u32 = 2;
pub const ANDROID_ALARM_ELAPSED_REALTIME: u32 = 3;
pub const ANDROID_ALARM_SYSTEMTIME: u32 = 4;
pub const ANDROID_ALARM_TYPE_COUNT: u32 = 5;

// ---------------------------------------------------------------------------
// RTC wake-up ioctls (`<linux/rtc.h>`)
// ---------------------------------------------------------------------------

pub const DEV_RTC: &str = "/dev/rtc";
pub const DEV_RTC0: &str = "/dev/rtc0";

pub const RTC_AIE_ON: u32 = 0x7001;
pub const RTC_AIE_OFF: u32 = 0x7002;
pub const RTC_UIE_ON: u32 = 0x7003;
pub const RTC_UIE_OFF: u32 = 0x7004;
pub const RTC_PIE_ON: u32 = 0x7005;
pub const RTC_PIE_OFF: u32 = 0x7006;
pub const RTC_WIE_ON: u32 = 0x700F;
pub const RTC_WIE_OFF: u32 = 0x7010;

// ---------------------------------------------------------------------------
// /sys/class/rtc/rtcN/wakealarm — a single Unix-time integer
// ---------------------------------------------------------------------------

pub const SYS_CLASS_RTC: &str = "/sys/class/rtc";
pub const RTC_WAKEALARM_ATTR: &str = "wakealarm";

/// Writing this sentinel disables a previously armed wakealarm.
pub const RTC_WAKEALARM_DISABLE_STR: &str = "0";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alarm_clock_ids_distinct() {
        // 8 and 9 — neighbours but distinct.
        assert_ne!(CLOCK_REALTIME_ALARM, CLOCK_BOOTTIME_ALARM);
        assert_eq!(CLOCK_BOOTTIME_ALARM, 9);
        assert_eq!(CLOCK_REALTIME_ALARM, 8);
    }

    #[test]
    fn test_dev_alarm_path() {
        assert_eq!(DEV_ALARM, "/dev/alarm");
    }

    #[test]
    fn test_android_alarm_types_dense_0_to_4() {
        let t = [
            ANDROID_ALARM_RTC_WAKEUP,
            ANDROID_ALARM_RTC,
            ANDROID_ALARM_ELAPSED_REALTIME_WAKEUP,
            ANDROID_ALARM_ELAPSED_REALTIME,
            ANDROID_ALARM_SYSTEMTIME,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(ANDROID_ALARM_TYPE_COUNT as usize, t.len());
    }

    #[test]
    fn test_rtc_ioctls_distinct_and_in_0x70xx_range() {
        let i = [
            RTC_AIE_ON,
            RTC_AIE_OFF,
            RTC_UIE_ON,
            RTC_UIE_OFF,
            RTC_PIE_ON,
            RTC_PIE_OFF,
            RTC_WIE_ON,
            RTC_WIE_OFF,
        ];
        for v in i {
            assert_eq!(v & 0xFF00, 0x7000);
        }
        // ON/OFF pairs are consecutive odd/even.
        assert_eq!(RTC_AIE_OFF, RTC_AIE_ON + 1);
        assert_eq!(RTC_UIE_OFF, RTC_UIE_ON + 1);
        assert_eq!(RTC_PIE_OFF, RTC_PIE_ON + 1);
        assert_eq!(RTC_WIE_OFF, RTC_WIE_ON + 1);
    }

    #[test]
    fn test_rtc_dev_paths_consistent() {
        // /dev/rtc is a symlink to /dev/rtcN on most systems.
        assert_eq!(DEV_RTC, "/dev/rtc");
        assert_eq!(DEV_RTC0, "/dev/rtc0");
        assert!(DEV_RTC0.starts_with(DEV_RTC));
    }

    #[test]
    fn test_wakealarm_sysfs() {
        assert_eq!(SYS_CLASS_RTC, "/sys/class/rtc");
        assert_eq!(RTC_WAKEALARM_ATTR, "wakealarm");
        // Disable is just the ASCII zero.
        assert_eq!(RTC_WAKEALARM_DISABLE_STR, "0");
    }
}
