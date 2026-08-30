//! `<linux/apm_bios.h>` — APM (Advanced Power Management) BIOS event codes.
//!
//! APM is the pre-ACPI power-management interface. Linux still
//! supports it through `/dev/apm_bios` and exports battery / state
//! info through `/proc/apm`. This module enumerates the codes and
//! state values commonly consulted by legacy userspace.

// ---------------------------------------------------------------------------
// Device and procfs paths
// ---------------------------------------------------------------------------

pub const DEV_APM_BIOS: &str = "/dev/apm_bios";
pub const PROC_APM: &str = "/proc/apm";

// ---------------------------------------------------------------------------
// AC line-status values (`/proc/apm` byte 5)
// ---------------------------------------------------------------------------

pub const APM_AC_OFFLINE: u8 = 0x00;
pub const APM_AC_ONLINE: u8 = 0x01;
pub const APM_AC_BACKUP: u8 = 0x02;
pub const APM_AC_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Battery-status values (`/proc/apm` byte 6)
// ---------------------------------------------------------------------------

pub const APM_BATT_HIGH: u8 = 0x00;
pub const APM_BATT_LOW: u8 = 0x01;
pub const APM_BATT_CRITICAL: u8 = 0x02;
pub const APM_BATT_CHARGING: u8 = 0x03;
pub const APM_BATT_ABSENT: u8 = 0x04;
pub const APM_BATT_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// APM event codes (`apm_event_t`)
// ---------------------------------------------------------------------------

pub const APM_SYS_STANDBY: u16 = 0x0001;
pub const APM_SYS_SUSPEND: u16 = 0x0002;
pub const APM_NORMAL_RESUME: u16 = 0x0003;
pub const APM_CRITICAL_RESUME: u16 = 0x0004;
pub const APM_LOW_BATTERY: u16 = 0x0005;
pub const APM_POWER_STATUS_CHANGE: u16 = 0x0006;
pub const APM_UPDATE_TIME: u16 = 0x0007;
pub const APM_CRITICAL_SUSPEND: u16 = 0x0008;
pub const APM_USER_STANDBY: u16 = 0x0009;
pub const APM_USER_SUSPEND: u16 = 0x000A;
pub const APM_STANDBY_RESUME: u16 = 0x000B;
pub const APM_CAPABILITY_CHANGE: u16 = 0x000C;

// ---------------------------------------------------------------------------
// APM BIOS revision (BCD)
// ---------------------------------------------------------------------------

pub const APM_VERSION_1_2: u16 = 0x0102;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths() {
        assert_eq!(DEV_APM_BIOS, "/dev/apm_bios");
        assert_eq!(PROC_APM, "/proc/apm");
    }

    #[test]
    fn test_ac_values_dense_low_then_unknown_high() {
        assert_eq!(APM_AC_OFFLINE, 0);
        assert_eq!(APM_AC_ONLINE, 1);
        assert_eq!(APM_AC_BACKUP, 2);
        assert_eq!(APM_AC_UNKNOWN, 0xFF);
    }

    #[test]
    fn test_battery_values_dense_low_then_unknown_high() {
        assert_eq!(APM_BATT_HIGH, 0);
        assert_eq!(APM_BATT_LOW, 1);
        assert_eq!(APM_BATT_CRITICAL, 2);
        assert_eq!(APM_BATT_CHARGING, 3);
        assert_eq!(APM_BATT_ABSENT, 4);
        assert_eq!(APM_BATT_UNKNOWN, 0xFF);
    }

    #[test]
    fn test_event_codes_dense_1_to_c() {
        let e = [
            APM_SYS_STANDBY,
            APM_SYS_SUSPEND,
            APM_NORMAL_RESUME,
            APM_CRITICAL_RESUME,
            APM_LOW_BATTERY,
            APM_POWER_STATUS_CHANGE,
            APM_UPDATE_TIME,
            APM_CRITICAL_SUSPEND,
            APM_USER_STANDBY,
            APM_USER_SUSPEND,
            APM_STANDBY_RESUME,
            APM_CAPABILITY_CHANGE,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_version_bcd_1_2() {
        // APM_VERSION_1_2 encodes "1.2" as 0x0102 (BCD-style).
        assert_eq!(APM_VERSION_1_2 >> 8, 1);
        assert_eq!(APM_VERSION_1_2 & 0xFF, 2);
    }
}
