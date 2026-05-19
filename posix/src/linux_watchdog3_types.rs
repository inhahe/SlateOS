//! `<linux/watchdog.h>` — Additional watchdog constants (part 3).
//!
//! Supplementary watchdog constants covering option flags,
//! ioctl commands, and boot status bits.

// ---------------------------------------------------------------------------
// Watchdog option flags
// ---------------------------------------------------------------------------

/// Overheat reset.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan fault.
pub const WDIOF_FANFAULT: u32 = 0x0002;
/// External relay 1.
pub const WDIOF_EXTERN1: u32 = 0x0004;
/// External relay 2.
pub const WDIOF_EXTERN2: u32 = 0x0008;
/// Power under voltage.
pub const WDIOF_POWERUNDER: u32 = 0x0010;
/// Card previously reset.
pub const WDIOF_CARDRESET: u32 = 0x0020;
/// Power over voltage.
pub const WDIOF_POWEROVER: u32 = 0x0040;
/// Set timeout.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Magic close.
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Pretimeout.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Always running.
pub const WDIOF_ALARMONLY: u32 = 0x0400;
/// Keepalive ping.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Watchdog ioctl commands
// ---------------------------------------------------------------------------

/// Get support info.
pub const WDIOC_GETSUPPORT: u32 = 0x80285700;
/// Get status.
pub const WDIOC_GETSTATUS: u32 = 0x80045701;
/// Get boot status.
pub const WDIOC_GETBOOTSTATUS: u32 = 0x80045702;
/// Get temperature.
pub const WDIOC_GETTEMP: u32 = 0x80045703;
/// Set options.
pub const WDIOC_SETOPTIONS: u32 = 0x80045704;
/// Keepalive.
pub const WDIOC_KEEPALIVE: u32 = 0x80045705;
/// Set timeout.
pub const WDIOC_SETTIMEOUT: u32 = 0xC0045706;
/// Get timeout.
pub const WDIOC_GETTIMEOUT: u32 = 0x80045707;
/// Set pretimeout.
pub const WDIOC_SETPRETIMEOUT: u32 = 0xC0045708;
/// Get pretimeout.
pub const WDIOC_GETPRETIMEOUT: u32 = 0x80045709;
/// Get time left.
pub const WDIOC_GETTIMELEFT: u32 = 0x8004570A;

// ---------------------------------------------------------------------------
// Watchdog control options
// ---------------------------------------------------------------------------

/// Disable watchdog.
pub const WDIOS_DISABLECARD: u32 = 0x0001;
/// Enable watchdog.
pub const WDIOS_ENABLECARD: u32 = 0x0002;
/// Turn off temperature panic.
pub const WDIOS_TEMPPANIC: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_flags_power_of_two() {
        let flags = [
            WDIOF_OVERHEAT, WDIOF_FANFAULT, WDIOF_EXTERN1,
            WDIOF_EXTERN2, WDIOF_POWERUNDER, WDIOF_CARDRESET,
            WDIOF_POWEROVER, WDIOF_SETTIMEOUT, WDIOF_MAGICCLOSE,
            WDIOF_PRETIMEOUT, WDIOF_ALARMONLY, WDIOF_KEEPALIVEPING,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            WDIOC_GETSUPPORT, WDIOC_GETSTATUS, WDIOC_GETBOOTSTATUS,
            WDIOC_GETTEMP, WDIOC_SETOPTIONS, WDIOC_KEEPALIVE,
            WDIOC_SETTIMEOUT, WDIOC_GETTIMEOUT, WDIOC_SETPRETIMEOUT,
            WDIOC_GETPRETIMEOUT, WDIOC_GETTIMELEFT,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_control_opts_no_overlap() {
        let opts = [WDIOS_DISABLECARD, WDIOS_ENABLECARD, WDIOS_TEMPPANIC];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }
}
