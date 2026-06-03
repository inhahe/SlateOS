//! `<termios.h>` / `<pty.h>` — pseudo-terminal master/slave ABI.
//!
//! PTYs are how shells, `screen`, `tmux`, `ssh`, and every terminal
//! emulator (xterm, gnome-terminal, alacritty) talk to programs that
//! expect a real tty. Linux uses Unix98 pty multiplexing via
//! `/dev/ptmx` → `ptsname(N)`; the constants here are the stable
//! ioctls and special device paths.

// ---------------------------------------------------------------------------
// Multiplexer / slave directories
// ---------------------------------------------------------------------------

/// Master multiplexer device — `open(2)` returns a new master fd.
pub const DEV_PTMX: &str = "/dev/ptmx";
/// Slave pty directory — `ptsname` returns `"/dev/pts/<N>"`.
pub const DEV_PTS_DIR: &str = "/dev/pts";
/// Mountpoint type for the slave pty filesystem.
pub const DEVPTS_FSTYPE: &str = "devpts";

// ---------------------------------------------------------------------------
// Unix98 pty ioctls (`TIOC*` from `<asm/ioctls.h>`)
// ---------------------------------------------------------------------------

pub const TIOCGPTN: u32 = 0x8004_5430;
pub const TIOCSPTLCK: u32 = 0x4004_5431;
pub const TIOCGPKT: u32 = 0x8004_5438;
pub const TIOCSIG: u32 = 0x4004_5436;
pub const TIOCPKT: u32 = 0x5470;
pub const TIOCGPTPEER: u32 = 0x5441;

// ---------------------------------------------------------------------------
// `TIOCPKT` (packet mode) status bits in the leading byte
// ---------------------------------------------------------------------------

pub const TIOCPKT_DATA: u8 = 0x00;
pub const TIOCPKT_FLUSHREAD: u8 = 0x01;
pub const TIOCPKT_FLUSHWRITE: u8 = 0x02;
pub const TIOCPKT_STOP: u8 = 0x04;
pub const TIOCPKT_START: u8 = 0x08;
pub const TIOCPKT_NOSTOP: u8 = 0x10;
pub const TIOCPKT_DOSTOP: u8 = 0x20;
pub const TIOCPKT_IOCTL: u8 = 0x40;

// ---------------------------------------------------------------------------
// `devpts` mount option defaults
// ---------------------------------------------------------------------------

/// Slave pty mode (`0620`) per Unix98.
pub const DEVPTS_MODE_DEFAULT: u16 = 0o620;
/// `ptmxmode=0666` — the default ptmx mode under devpts >= 2.
pub const DEVPTS_PTMX_MODE_DEFAULT: u16 = 0o666;
/// Maximum number of pty pairs (`pty.max` sysctl default).
pub const DEVPTS_PTY_MAX_DEFAULT: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_and_slave_paths() {
        assert_eq!(DEV_PTMX, "/dev/ptmx");
        assert_eq!(DEV_PTS_DIR, "/dev/pts");
        assert_eq!(DEVPTS_FSTYPE, "devpts");
        assert!(DEV_PTMX.starts_with("/dev/"));
        assert!(DEV_PTS_DIR.starts_with("/dev/"));
    }

    #[test]
    fn test_packet_status_bits_distinct_single_bit() {
        // The "status" bits used in TIOCPKT mode are all single bits.
        let p = [
            TIOCPKT_FLUSHREAD,
            TIOCPKT_FLUSHWRITE,
            TIOCPKT_STOP,
            TIOCPKT_START,
            TIOCPKT_NOSTOP,
            TIOCPKT_DOSTOP,
            TIOCPKT_IOCTL,
        ];
        let mut or = 0u8;
        for v in p {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x7F);
        // DATA is the no-event sentinel.
        assert_eq!(TIOCPKT_DATA, 0);
    }

    #[test]
    fn test_unix98_ioctls_distinct() {
        let i = [TIOCGPTN, TIOCSPTLCK, TIOCGPKT, TIOCSIG, TIOCPKT, TIOCGPTPEER];
        for a in 0..i.len() {
            for b in (a + 1)..i.len() {
                assert_ne!(i[a], i[b]);
            }
        }
    }

    #[test]
    fn test_devpts_default_modes() {
        // Unix98 slave perms are 0620.
        assert_eq!(DEVPTS_MODE_DEFAULT, 0o620);
        // ptmx default 0666 since devpts v2.
        assert_eq!(DEVPTS_PTMX_MODE_DEFAULT, 0o666);
    }

    #[test]
    fn test_pty_max_default_is_4096() {
        assert_eq!(DEVPTS_PTY_MAX_DEFAULT, 4096);
        assert!(DEVPTS_PTY_MAX_DEFAULT.is_power_of_two());
    }
}
