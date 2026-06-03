//! `<linux/apm_bios.h>` — Advanced Power Management BIOS event constants.
//!
//! APM is a legacy x86 power-management interface (`/dev/apm_bios`).
//! It predates ACPI but remains usable on some embedded x86 and
//! retro/test systems. Userspace `apmd`, busybox `apm`, and similar
//! tools poll the device for events and call ioctls to suspend or
//! query battery status using these constants.

// ---------------------------------------------------------------------------
// APM event codes (returned by read(2) on /dev/apm_bios)
// ---------------------------------------------------------------------------

/// Standby request.
pub const APM_SYS_STANDBY: u16 = 0x0001;
/// Suspend request.
pub const APM_SYS_SUSPEND: u16 = 0x0002;
/// Normal-resume notification.
pub const APM_NORMAL_RESUME: u16 = 0x0003;
/// Critical-resume notification.
pub const APM_CRITICAL_RESUME: u16 = 0x0004;
/// Battery status change.
pub const APM_LOW_BATTERY: u16 = 0x0005;
/// Power status change.
pub const APM_POWER_STATUS_CHANGE: u16 = 0x0006;
/// Update time after wake.
pub const APM_UPDATE_TIME: u16 = 0x0007;
/// Critical-suspend (no user warning possible).
pub const APM_CRITICAL_SUSPEND: u16 = 0x0008;
/// User-initiated standby.
pub const APM_USER_STANDBY: u16 = 0x0009;
/// User-initiated suspend.
pub const APM_USER_SUSPEND: u16 = 0x000a;
/// System standby resume.
pub const APM_STANDBY_RESUME: u16 = 0x000b;
/// Capabilities changed notification (APM 1.2).
pub const APM_CAPABILITY_CHANGE: u16 = 0x000c;

// ---------------------------------------------------------------------------
// AC line status byte
// ---------------------------------------------------------------------------

/// AC is offline (running on battery).
pub const APM_AC_OFFLINE: u8 = 0;
/// AC is online.
pub const APM_AC_ONLINE: u8 = 1;
/// AC backup power (UPS) supplying.
pub const APM_AC_BACKUP: u8 = 2;
/// AC status unknown.
pub const APM_AC_UNKNOWN: u8 = 0xff;

// ---------------------------------------------------------------------------
// Battery status byte
// ---------------------------------------------------------------------------

/// Battery is high.
pub const APM_BATTERY_STATUS_HIGH: u8 = 0;
/// Battery is low.
pub const APM_BATTERY_STATUS_LOW: u8 = 1;
/// Battery is critical.
pub const APM_BATTERY_STATUS_CRITICAL: u8 = 2;
/// Battery is charging.
pub const APM_BATTERY_STATUS_CHARGING: u8 = 3;
/// No system battery (e.g., desktop on AC).
pub const APM_BATTERY_STATUS_NOT_PRESENT: u8 = 4;
/// Battery status unknown.
pub const APM_BATTERY_STATUS_UNKNOWN: u8 = 0xff;

// ---------------------------------------------------------------------------
// ioctl base (APM_IOC = 'A')
// ---------------------------------------------------------------------------

/// ioctl group letter for /dev/apm_bios.
pub const APM_IOC_BASE: u8 = b'A';

/// `APM_IOC_STANDBY` — request system standby.
pub const APM_IOC_STANDBY: u32 = 0x4101;
/// `APM_IOC_SUSPEND` — request full suspend.
pub const APM_IOC_SUSPEND: u32 = 0x4102;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_codes_distinct_and_in_apm_range() {
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
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
            // All APM event codes fit in a single byte (BIOS returns them
            // in AL/AH pair).
            assert!(e[i] <= 0xff);
        }
    }

    #[test]
    fn test_ac_states_distinct() {
        let a = [
            APM_AC_OFFLINE,
            APM_AC_ONLINE,
            APM_AC_BACKUP,
            APM_AC_UNKNOWN,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        // Unknown is the sentinel 0xff so a zeroed buffer doesn't lie
        // about AC status.
        assert_eq!(APM_AC_UNKNOWN, 0xff);
    }

    #[test]
    fn test_battery_states_distinct_and_unknown_is_ff() {
        let b = [
            APM_BATTERY_STATUS_HIGH,
            APM_BATTERY_STATUS_LOW,
            APM_BATTERY_STATUS_CRITICAL,
            APM_BATTERY_STATUS_CHARGING,
            APM_BATTERY_STATUS_NOT_PRESENT,
            APM_BATTERY_STATUS_UNKNOWN,
        ];
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
        assert_eq!(APM_BATTERY_STATUS_UNKNOWN, 0xff);
    }

    #[test]
    fn test_ioctl_numbers_share_group() {
        assert_ne!(APM_IOC_STANDBY, APM_IOC_SUSPEND);
        assert_eq!(APM_IOC_BASE, b'A');
        // ioctl encoding: byte 1 holds the group letter ('A' == 0x41).
        assert_eq!((APM_IOC_STANDBY >> 8) & 0xff, u32::from(APM_IOC_BASE));
        assert_eq!((APM_IOC_SUSPEND >> 8) & 0xff, u32::from(APM_IOC_BASE));
    }
}
