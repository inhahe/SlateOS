//! `<linux/watchdog.h>` — userspace API for hardware/software watchdogs.
//!
//! `/dev/watchdogN` is the canonical Linux interface. Userspace pings
//! the device (a write or `WDIOC_KEEPALIVE` ioctl) before the timeout
//! elapses; failure to do so causes the watchdog to reset the system.
//! Configuration (timeout, pretimeout, status, options) is performed
//! via the WDIOC_* ioctls below.

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

/// Primary watchdog device.
pub const WATCHDOG_DEV_PATH: &str = "/dev/watchdog";
/// `/dev/watchdog0` — first numbered device.
pub const WATCHDOG_DEV_PATH0: &str = "/dev/watchdog0";
/// Sysfs class root for watchdog devices.
pub const WATCHDOG_SYSFS_ROOT: &str = "/sys/class/watchdog";

// ---------------------------------------------------------------------------
// ioctl base letter
// ---------------------------------------------------------------------------

/// Ioctl group letter used by `WDIOC_*` (`'W'` per `<linux/watchdog.h>`).
pub const WATCHDOG_IOCTL_BASE: u8 = b'W';

// ---------------------------------------------------------------------------
// WDIOC_* ioctl numbers
// ---------------------------------------------------------------------------
//
// These are the raw numeric values that `<linux/watchdog.h>` expands to
// after the _IOR/_IOWR macros are applied on x86_64. Userspace code that
// wraps `ioctl(fd, WDIOC_*, ...)` only needs the final 32-bit constant.

/// `WDIOC_GETSUPPORT` — fill `struct watchdog_info` describing the device.
pub const WDIOC_GETSUPPORT: u32 = 0x8028_5700;
/// `WDIOC_GETSTATUS` — read current internal flags (WDIOF_* bits).
pub const WDIOC_GETSTATUS: u32 = 0x8004_5701;
/// `WDIOC_GETBOOTSTATUS` — read why the last boot occurred (WDIOF_* bits).
pub const WDIOC_GETBOOTSTATUS: u32 = 0x8004_5702;
/// `WDIOC_GETTEMP` — read current temperature in degrees Fahrenheit.
pub const WDIOC_GETTEMP: u32 = 0x8004_5703;
/// `WDIOC_SETOPTIONS` — change runtime behavior (WDIOS_* bits).
pub const WDIOC_SETOPTIONS: u32 = 0x8004_5704;
/// `WDIOC_KEEPALIVE` — restart the watchdog timer.
pub const WDIOC_KEEPALIVE: u32 = 0x8004_5705;
/// `WDIOC_SETTIMEOUT` — set the keepalive interval in seconds.
pub const WDIOC_SETTIMEOUT: u32 = 0xc004_5706;
/// `WDIOC_GETTIMEOUT` — read the current keepalive interval in seconds.
pub const WDIOC_GETTIMEOUT: u32 = 0x8004_5707;
/// `WDIOC_SETPRETIMEOUT` — set the pre-timeout (warning) interval in seconds.
pub const WDIOC_SETPRETIMEOUT: u32 = 0xc004_5708;
/// `WDIOC_GETPRETIMEOUT` — read the current pre-timeout interval in seconds.
pub const WDIOC_GETPRETIMEOUT: u32 = 0x8004_5709;
/// `WDIOC_GETTIMELEFT` — seconds remaining until the watchdog fires.
pub const WDIOC_GETTIMELEFT: u32 = 0x8004_570a;

// ---------------------------------------------------------------------------
// WDIOF_* — status flags returned by GETSUPPORT, GETSTATUS, GETBOOTSTATUS
// ---------------------------------------------------------------------------

/// Reset due to CPU overheat.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan failed.
pub const WDIOF_FANFAULT: u32 = 0x0002;
/// External relay 1 triggered.
pub const WDIOF_EXTERN1: u32 = 0x0004;
/// External relay 2 triggered.
pub const WDIOF_EXTERN2: u32 = 0x0008;
/// Power input went bad (under-voltage / brown-out).
pub const WDIOF_POWERUNDER: u32 = 0x0010;
/// Card previously reset the CPU.
pub const WDIOF_CARDRESET: u32 = 0x0020;
/// Power over-voltage.
pub const WDIOF_POWEROVER: u32 = 0x0040;
/// Set timeout (in seconds) is supported.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Watchdog supports magic-close character (`'V'`) to disable on close.
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Watchdog supports a pretimeout warning interval.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Watchdog can be ALARM-driven (ALARMONLY).
pub const WDIOF_ALARMONLY: u32 = 0x0400;
/// Watchdog can return seconds-left via WDIOC_GETTIMELEFT.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;

// ---------------------------------------------------------------------------
// WDIOS_* — option bits accepted by WDIOC_SETOPTIONS
// ---------------------------------------------------------------------------

/// Turn the watchdog timer off.
pub const WDIOS_DISABLECARD: u32 = 0x0001;
/// Turn the watchdog timer on.
pub const WDIOS_ENABLECARD: u32 = 0x0002;
/// Kernel panic when temperature trip point is reached.
pub const WDIOS_TEMPPANIC: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Magic close character
// ---------------------------------------------------------------------------

/// Writing this byte before closing tells the driver to disarm the
/// watchdog at close time (rather than continuing to count down).
pub const WATCHDOG_MAGIC_CLOSE: u8 = b'V';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_base_letter() {
        assert_eq!(WATCHDOG_IOCTL_BASE, b'W');
    }

    #[test]
    fn test_ioctl_numbers_distinct() {
        let ioctls = [
            WDIOC_GETSUPPORT,
            WDIOC_GETSTATUS,
            WDIOC_GETBOOTSTATUS,
            WDIOC_GETTEMP,
            WDIOC_SETOPTIONS,
            WDIOC_KEEPALIVE,
            WDIOC_SETTIMEOUT,
            WDIOC_GETTIMEOUT,
            WDIOC_SETPRETIMEOUT,
            WDIOC_GETPRETIMEOUT,
            WDIOC_GETTIMELEFT,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [
            WDIOF_OVERHEAT,
            WDIOF_FANFAULT,
            WDIOF_EXTERN1,
            WDIOF_EXTERN2,
            WDIOF_POWERUNDER,
            WDIOF_CARDRESET,
            WDIOF_POWEROVER,
            WDIOF_SETTIMEOUT,
            WDIOF_MAGICCLOSE,
            WDIOF_PRETIMEOUT,
            WDIOF_ALARMONLY,
            WDIOF_KEEPALIVEPING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_setoptions_bits_no_overlap() {
        assert_eq!(WDIOS_DISABLECARD & WDIOS_ENABLECARD, 0);
        assert_eq!(WDIOS_ENABLECARD & WDIOS_TEMPPANIC, 0);
        assert_eq!(WDIOS_DISABLECARD & WDIOS_TEMPPANIC, 0);
    }

    #[test]
    fn test_magic_close_character() {
        assert_eq!(WATCHDOG_MAGIC_CLOSE, b'V');
    }

    #[test]
    fn test_dev_paths_nonempty() {
        assert!(!WATCHDOG_DEV_PATH.is_empty());
        assert!(WATCHDOG_DEV_PATH.starts_with("/dev/"));
        assert!(WATCHDOG_DEV_PATH0.starts_with("/dev/watchdog"));
        assert!(WATCHDOG_SYSFS_ROOT.starts_with("/sys/"));
    }
}
