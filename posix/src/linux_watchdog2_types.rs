//! `<linux/watchdog.h>` — Additional watchdog constants.
//!
//! Supplementary watchdog constants covering watchdog info flags,
//! watchdog options, and pretimeout governors.

// ---------------------------------------------------------------------------
// Watchdog info status flags (WDIOF_*)
// ---------------------------------------------------------------------------

/// Overheat warning.
pub const WDIOF_OVERHEAT: u32 = 0x0001;
/// Fan fault detected.
pub const WDIOF_FANFAULT: u32 = 0x0002;
/// External relay 1.
pub const WDIOF_EXTERN1: u32 = 0x0004;
/// External relay 2.
pub const WDIOF_EXTERN2: u32 = 0x0008;
/// Power under voltage.
pub const WDIOF_POWERUNDER: u32 = 0x0010;
/// Card/board reset.
pub const WDIOF_CARDRESET: u32 = 0x0020;
/// Power over voltage.
pub const WDIOF_POWEROVER: u32 = 0x0040;
/// Keep alive ping supported.
pub const WDIOF_KEEPALIVEPING: u32 = 0x8000;
/// Set timeout supported.
pub const WDIOF_SETTIMEOUT: u32 = 0x0080;
/// Magic close supported.
pub const WDIOF_MAGICCLOSE: u32 = 0x0100;
/// Pretimeout supported.
pub const WDIOF_PRETIMEOUT: u32 = 0x0200;
/// Always running.
pub const WDIOF_ALARMONLY: u32 = 0x0400;

// ---------------------------------------------------------------------------
// Watchdog options (WDIOS_*)
// ---------------------------------------------------------------------------

/// Disable card.
pub const WDIOS_DISABLECARD: u32 = 0x0001;
/// Enable card.
pub const WDIOS_ENABLECARD: u32 = 0x0002;
/// Temperature panic.
pub const WDIOS_TEMPPANIC: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Watchdog ioctl commands
// ---------------------------------------------------------------------------

/// Get support info.
pub const WDIOC_GETSUPPORT: u32 = 0x80280000;
/// Get status.
pub const WDIOC_GETSTATUS: u32 = 0x80040001;
/// Get boot status.
pub const WDIOC_GETBOOTSTATUS: u32 = 0x80040002;
/// Get temperature.
pub const WDIOC_GETTEMP: u32 = 0x80040003;
/// Set options.
pub const WDIOC_SETOPTIONS: u32 = 0x80040004;
/// Keep alive.
pub const WDIOC_KEEPALIVE: u32 = 0x80040005;
/// Set timeout.
pub const WDIOC_SETTIMEOUT: u32 = 0xC0040006;
/// Get timeout.
pub const WDIOC_GETTIMEOUT: u32 = 0x80040007;
/// Set pretimeout.
pub const WDIOC_SETPRETIMEOUT: u32 = 0xC0040008;
/// Get pretimeout.
pub const WDIOC_GETPRETIMEOUT: u32 = 0x80040009;
/// Get timeleft.
pub const WDIOC_GETTIMELEFT: u32 = 0x8004000A;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_flags_distinct() {
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
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
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
    fn test_disable_enable_no_overlap() {
        assert_eq!(WDIOS_DISABLECARD & WDIOS_ENABLECARD, 0);
    }
}
