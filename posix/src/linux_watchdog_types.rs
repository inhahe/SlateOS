//! `<linux/watchdog.h>` — Hardware watchdog timer constants.
//!
//! Hardware watchdog timers reset the system if software stops
//! responding. Userspace must periodically write to /dev/watchdog
//! (or send WDIOC_KEEPALIVE ioctl) to prevent timeout. If the
//! process dies or hangs, the watchdog triggers a system reset,
//! ensuring recovery from software lockups.

// ---------------------------------------------------------------------------
// Watchdog ioctl commands
// ---------------------------------------------------------------------------

/// Get watchdog support/status flags.
pub const WDIOC_GETSUPPORT: u32 = 0x80285700;
/// Get watchdog status.
pub const WDIOC_GETSTATUS: u32 = 0x80045701;
/// Get boot status (reason for last reset).
pub const WDIOC_GETBOOTSTATUS: u32 = 0x80045702;
/// Get temperature.
pub const WDIOC_GETTEMP: u32 = 0x80045703;
/// Set watchdog options.
pub const WDIOC_SETOPTIONS: u32 = 0x40045704;
/// Send keepalive ping.
pub const WDIOC_KEEPALIVE: u32 = 0x80045705;
/// Set timeout (seconds).
pub const WDIOC_SETTIMEOUT: u32 = 0xC0045706;
/// Get timeout (seconds).
pub const WDIOC_GETTIMEOUT: u32 = 0x80045707;
/// Set pretimeout (seconds before actual timeout).
pub const WDIOC_SETPRETIMEOUT: u32 = 0xC0045708;
/// Get pretimeout.
pub const WDIOC_GETPRETIMEOUT: u32 = 0x80045709;

// ---------------------------------------------------------------------------
// Watchdog option flags (WDIOC_SETOPTIONS)
// ---------------------------------------------------------------------------

/// Disable the watchdog timer.
pub const WDIOS_DISABLECARD: u32 = 0x0001;
/// Enable the watchdog timer.
pub const WDIOS_ENABLECARD: u32 = 0x0002;
/// Trigger a temperature panic.
pub const WDIOS_TEMPPANIC: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Watchdog status flags (WDIOF_*)
// ---------------------------------------------------------------------------

/// Overheat detected.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan failure.
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
/// Set timeout supported.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Magic close supported.
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Pretimeout supported.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Keepalive ping supported.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_flags_no_overlap() {
        let opts = [WDIOS_DISABLECARD, WDIOS_ENABLECARD, WDIOS_TEMPPANIC];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [
            WDIOF_OVERHEAT, WDIOF_FANFAULT, WDIOF_EXTERN1,
            WDIOF_EXTERN2, WDIOF_POWERUNDER, WDIOF_CARDRESET,
            WDIOF_POWEROVER, WDIOF_SETTIMEOUT, WDIOF_MAGICCLOSE,
            WDIOF_PRETIMEOUT, WDIOF_KEEPALIVEPING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            WDIOC_GETSUPPORT, WDIOC_GETSTATUS, WDIOC_GETBOOTSTATUS,
            WDIOC_GETTEMP, WDIOC_SETOPTIONS, WDIOC_KEEPALIVE,
            WDIOC_SETTIMEOUT, WDIOC_GETTIMEOUT,
            WDIOC_SETPRETIMEOUT, WDIOC_GETPRETIMEOUT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
