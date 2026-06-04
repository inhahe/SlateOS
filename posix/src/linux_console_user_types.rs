//! `<linux/console.h>` — Console driver and VT ioctl constants.
//!
//! The Linux console layer multiplexes between virtual terminals,
//! framebuffer consoles, and serial consoles. Userspace interacts
//! primarily through /dev/tty[N], /dev/console, and KDxxx ioctls.

// ---------------------------------------------------------------------------
// Console device paths
// ---------------------------------------------------------------------------

pub const CONSOLE_DEV_CONSOLE: &str = "/dev/console";
pub const CONSOLE_DEV_TTY: &str = "/dev/tty";
pub const CONSOLE_DEV_TTY0: &str = "/dev/tty0";
pub const CONSOLE_DEV_VCS: &str = "/dev/vcs";
pub const CONSOLE_DEV_VCSA: &str = "/dev/vcsa";

// ---------------------------------------------------------------------------
// Virtual terminal count
// ---------------------------------------------------------------------------

/// Default number of virtual terminals.
pub const MAX_NR_CONSOLES: u32 = 63;

/// Foreground VT index (1-based; VT 0 is /dev/tty0 → current).
pub const VT_FOREGROUND_MIN: u32 = 1;
pub const VT_FOREGROUND_MAX: u32 = 63;

// ---------------------------------------------------------------------------
// KD console mode (KDSETMODE/KDGETMODE values)
// ---------------------------------------------------------------------------

pub const KD_TEXT: u32 = 0x00;
pub const KD_GRAPHICS: u32 = 0x01;
pub const KD_TEXT0: u32 = 0x02;
pub const KD_TEXT1: u32 = 0x03;

// ---------------------------------------------------------------------------
// Console blanking flags (TIOCLINUX subcode 1)
// ---------------------------------------------------------------------------

pub const TIOCL_SETSEL: u8 = 2;
pub const TIOCL_PASTESEL: u8 = 3;
pub const TIOCL_UNBLANKSCREEN: u8 = 4;
pub const TIOCL_SELLOADLUT: u8 = 5;
pub const TIOCL_GETSHIFTSTATE: u8 = 6;
pub const TIOCL_GETMOUSEREPORTING: u8 = 7;
pub const TIOCL_BLANKSCREEN: u8 = 14;

// ---------------------------------------------------------------------------
// Console message levels (printk)
// ---------------------------------------------------------------------------

pub const CONSOLE_LOGLEVEL_EMERG: u32 = 0;
pub const CONSOLE_LOGLEVEL_ALERT: u32 = 1;
pub const CONSOLE_LOGLEVEL_CRIT: u32 = 2;
pub const CONSOLE_LOGLEVEL_ERR: u32 = 3;
pub const CONSOLE_LOGLEVEL_WARNING: u32 = 4;
pub const CONSOLE_LOGLEVEL_NOTICE: u32 = 5;
pub const CONSOLE_LOGLEVEL_INFO: u32 = 6;
pub const CONSOLE_LOGLEVEL_DEBUG: u32 = 7;

/// Default loglevel — show warnings and worse.
pub const CONSOLE_LOGLEVEL_DEFAULT: u32 = 7;
pub const CONSOLE_LOGLEVEL_QUIET: u32 = 4;
pub const CONSOLE_LOGLEVEL_SILENT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_paths_under_dev() {
        for d in [
            CONSOLE_DEV_CONSOLE,
            CONSOLE_DEV_TTY,
            CONSOLE_DEV_TTY0,
            CONSOLE_DEV_VCS,
            CONSOLE_DEV_VCSA,
        ] {
            assert!(d.starts_with("/dev/"));
        }
    }

    #[test]
    fn test_max_nr_consoles_63() {
        assert_eq!(MAX_NR_CONSOLES, 63);
        assert_eq!(VT_FOREGROUND_MAX, MAX_NR_CONSOLES);
        assert_eq!(VT_FOREGROUND_MIN, 1);
    }

    #[test]
    fn test_kd_modes_dense() {
        assert_eq!(KD_TEXT, 0);
        assert_eq!(KD_GRAPHICS, 1);
        assert_eq!(KD_TEXT0, 2);
        assert_eq!(KD_TEXT1, 3);
    }

    #[test]
    fn test_tiocl_codes_distinct() {
        let codes = [
            TIOCL_SETSEL,
            TIOCL_PASTESEL,
            TIOCL_UNBLANKSCREEN,
            TIOCL_SELLOADLUT,
            TIOCL_GETSHIFTSTATE,
            TIOCL_GETMOUSEREPORTING,
            TIOCL_BLANKSCREEN,
        ];
        for (i, &x) in codes.iter().enumerate() {
            for &y in &codes[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // BLANKSCREEN is the highest at 14.
        assert_eq!(TIOCL_BLANKSCREEN, 14);
    }

    #[test]
    fn test_loglevels_dense_0_to_7() {
        let l = [
            CONSOLE_LOGLEVEL_EMERG,
            CONSOLE_LOGLEVEL_ALERT,
            CONSOLE_LOGLEVEL_CRIT,
            CONSOLE_LOGLEVEL_ERR,
            CONSOLE_LOGLEVEL_WARNING,
            CONSOLE_LOGLEVEL_NOTICE,
            CONSOLE_LOGLEVEL_INFO,
            CONSOLE_LOGLEVEL_DEBUG,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_loglevel_aliases_match_canonical() {
        assert_eq!(CONSOLE_LOGLEVEL_QUIET, CONSOLE_LOGLEVEL_WARNING);
        assert_eq!(CONSOLE_LOGLEVEL_SILENT, CONSOLE_LOGLEVEL_EMERG);
        assert_eq!(CONSOLE_LOGLEVEL_DEFAULT, CONSOLE_LOGLEVEL_DEBUG);
    }
}
