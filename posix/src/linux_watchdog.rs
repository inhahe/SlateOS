//! `<linux/watchdog.h>` — watchdog timer interface.
//!
//! Provides ioctl constants and structures for the hardware/software
//! watchdog timer (`/dev/watchdog`).

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Get watchdog support info.
pub const WDIOC_GETSUPPORT: u64 = 0x8028_5700;
/// Get current status.
pub const WDIOC_GETSTATUS: u64 = 0x8004_5701;
/// Get boot status (reason for last boot).
pub const WDIOC_GETBOOTSTATUS: u64 = 0x8004_5702;
/// Get temperature (in Fahrenheit).
pub const WDIOC_GETTEMP: u64 = 0x8004_5703;
/// Set options (enable/disable).
pub const WDIOC_SETOPTIONS: u64 = 0x8004_5704;
/// Keepalive ping.
pub const WDIOC_KEEPALIVE: u64 = 0x8004_5705;
/// Set timeout (seconds).
pub const WDIOC_SETTIMEOUT: u64 = 0xC004_5706;
/// Get timeout (seconds).
pub const WDIOC_GETTIMEOUT: u64 = 0x8004_5707;
/// Set pre-timeout (seconds).
pub const WDIOC_SETPRETIMEOUT: u64 = 0xC004_5708;
/// Get pre-timeout (seconds).
pub const WDIOC_GETPRETIMEOUT: u64 = 0x8004_5709;
/// Get time left (seconds).
pub const WDIOC_GETTIMELEFT: u64 = 0x8004_570A;

// ---------------------------------------------------------------------------
// Watchdog option flags
// ---------------------------------------------------------------------------

/// Disable the watchdog (if supported).
pub const WDIOS_DISABLECARD: i32 = 0x0001;
/// Enable the watchdog.
pub const WDIOS_ENABLECARD: i32 = 0x0002;
/// Enable temperature panic.
pub const WDIOS_TEMPPANIC: i32 = 0x0004;

// ---------------------------------------------------------------------------
// Watchdog status flags
// ---------------------------------------------------------------------------

/// Over-temperature detected.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan fault detected.
pub const WDIOF_FANFAULT: u32 = 0x0002;
/// External relay 1.
pub const WDIOF_EXTERN1: u32 = 0x0004;
/// External relay 2.
pub const WDIOF_EXTERN2: u32 = 0x0008;
/// Power under voltage.
pub const WDIOF_POWERUNDER: u32 = 0x0010;
/// Card reset last reboot.
pub const WDIOF_CARDRESET: u32 = 0x0020;
/// Power over voltage.
pub const WDIOF_POWEROVER: u32 = 0x0040;
/// Set timeout supported.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Magic close supported (write 'V' before close to disable).
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Pre-timeout interrupt.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Keepalive ping supported.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Watchdog info struct
// ---------------------------------------------------------------------------

/// Watchdog identification and support info.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WatchdogInfo {
    /// Options supported (WDIOF_* bitmask).
    pub options: u32,
    /// Firmware version.
    pub firmware_version: u32,
    /// Device identity string.
    pub identity: [u8; 32],
}

impl WatchdogInfo {
    /// Create a zeroed `WatchdogInfo`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Magic character to write before closing to disable the watchdog.
pub const WATCHDOG_MAGIC_CLOSE: u8 = b'V';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watchdog_info_size() {
        // 4 + 4 + 32 = 40 bytes.
        assert_eq!(core::mem::size_of::<WatchdogInfo>(), 40);
    }

    #[test]
    fn test_watchdog_info_zeroed() {
        let info = WatchdogInfo::zeroed();
        assert_eq!(info.options, 0);
        assert_eq!(info.firmware_version, 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
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
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_option_flags_are_bits() {
        assert_eq!(WDIOS_DISABLECARD, 1);
        assert_eq!(WDIOS_ENABLECARD, 2);
        assert_eq!(WDIOS_TEMPPANIC, 4);
    }

    #[test]
    fn test_status_flags_are_bits() {
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
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "Status flags must not overlap");
            }
        }
    }

    #[test]
    fn test_magic_close() {
        assert_eq!(WATCHDOG_MAGIC_CLOSE, b'V');
    }
}
