//! `<linux/watchdog.h>` — Watchdog timer constants.
//!
//! Hardware watchdog timers are countdown timers that reset the system
//! if software fails to periodically "pet" (reset) them. They detect
//! system hangs, kernel panics, and infinite loops. The Linux watchdog
//! subsystem provides /dev/watchdog for userspace watchdog daemons.

// ---------------------------------------------------------------------------
// Watchdog ioctl commands
// ---------------------------------------------------------------------------

/// Get watchdog support/options.
pub const WDIOC_GETSUPPORT: u32 = 0x80280000;
/// Get status.
pub const WDIOC_GETSTATUS: u32 = 0x80040001;
/// Get boot status (cause of last reboot).
pub const WDIOC_GETBOOTSTATUS: u32 = 0x80040002;
/// Get temperature.
pub const WDIOC_GETTEMP: u32 = 0x80040003;
/// Set options (enable/disable).
pub const WDIOC_SETOPTIONS: u32 = 0x80040004;
/// Keep alive (pet the watchdog).
pub const WDIOC_KEEPALIVE: u32 = 0x80040005;
/// Set timeout (seconds).
pub const WDIOC_SETTIMEOUT: u32 = 0xC0040006;
/// Get timeout (seconds).
pub const WDIOC_GETTIMEOUT: u32 = 0x80040007;
/// Set pretimeout (seconds before timeout for warning).
pub const WDIOC_SETPRETIMEOUT: u32 = 0xC0040008;
/// Get pretimeout.
pub const WDIOC_GETPRETIMEOUT: u32 = 0x80040009;
/// Get time left (seconds until timeout).
pub const WDIOC_GETTIMELEFT: u32 = 0x8004000A;

// ---------------------------------------------------------------------------
// Watchdog option flags (WDIOC_SETOPTIONS)
// ---------------------------------------------------------------------------

/// Disable the watchdog.
pub const WDIOS_DISABLECARD: u32 = 0x0001;
/// Enable the watchdog.
pub const WDIOS_ENABLECARD: u32 = 0x0002;
/// Trigger temperature panic.
pub const WDIOS_TEMPPANIC: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Watchdog capability/info flags
// ---------------------------------------------------------------------------

/// Overheat trip detected.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan fault detected.
pub const WDIOF_FANFAULT: u32 = 0x0002;
/// External relay 1.
pub const WDIOF_EXTERN1: u32 = 0x0004;
/// External relay 2.
pub const WDIOF_EXTERN2: u32 = 0x0008;
/// Power under voltage.
pub const WDIOF_POWERUNDER: u32 = 0x0010;
/// Card previously reset CPU.
pub const WDIOF_CARDRESET: u32 = 0x0020;
/// Power over voltage.
pub const WDIOF_POWEROVER: u32 = 0x0040;
/// Timeout is settable.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Supports magic close (write 'V' to close).
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Pretimeout supported.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Keep-alive ping supported.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Magic close character
// ---------------------------------------------------------------------------

/// Magic character to safely close watchdog ('V').
pub const WATCHDOG_MAGIC_CLOSE: u8 = b'V';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
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
    fn test_options_distinct() {
        let opts = [WDIOS_DISABLECARD, WDIOS_ENABLECARD, WDIOS_TEMPPANIC];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_info_flags_no_overlap() {
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
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_magic_close() {
        assert_eq!(WATCHDOG_MAGIC_CLOSE, b'V');
    }
}
