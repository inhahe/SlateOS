//! `<sys/ioctl.h>` / `<asm-generic/ioctls.h>` — TTY ioctls.
//!
//! The `TIOC*` numbers control terminal lines: getting/setting termios
//! state, sending breaks, querying window size, managing the
//! controlling terminal. Every shell, screen lock, `tput`, and curses
//! program uses them.

// ---------------------------------------------------------------------------
// `TCGETS` / `TCSETS` family — wraps `tcgetattr`/`tcsetattr`
// ---------------------------------------------------------------------------

pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TCSETSW: u32 = 0x5403;
pub const TCSETSF: u32 = 0x5404;
pub const TCGETA: u32 = 0x5405;
pub const TCSETA: u32 = 0x5406;
pub const TCSETAW: u32 = 0x5407;
pub const TCSETAF: u32 = 0x5408;

// ---------------------------------------------------------------------------
// Line control / break / drain / flow
// ---------------------------------------------------------------------------

pub const TCSBRK: u32 = 0x5409;
pub const TCXONC: u32 = 0x540A;
pub const TCFLSH: u32 = 0x540B;
pub const TIOCSCTTY: u32 = 0x540E;
pub const TIOCGPGRP: u32 = 0x540F;
pub const TIOCSPGRP: u32 = 0x5410;
pub const TIOCOUTQ: u32 = 0x5411;
pub const TIOCSTI: u32 = 0x5412;

// ---------------------------------------------------------------------------
// Window size — `struct winsize`
// ---------------------------------------------------------------------------

pub const TIOCGWINSZ: u32 = 0x5413;
pub const TIOCSWINSZ: u32 = 0x5414;

// ---------------------------------------------------------------------------
// Modem / line status
// ---------------------------------------------------------------------------

pub const TIOCMGET: u32 = 0x5415;
pub const TIOCMBIS: u32 = 0x5416;
pub const TIOCMBIC: u32 = 0x5417;
pub const TIOCMSET: u32 = 0x5418;
pub const TIOCGSOFTCAR: u32 = 0x5419;
pub const TIOCSSOFTCAR: u32 = 0x541A;
pub const FIONREAD: u32 = 0x541B;
pub const TIOCEXCL: u32 = 0x540C;
pub const TIOCNXCL: u32 = 0x540D;
pub const TIOCNOTTY: u32 = 0x5422;

// ---------------------------------------------------------------------------
// Modem-status bits (`TIOCMGET`/`TIOCMSET` argument)
// ---------------------------------------------------------------------------

pub const TIOCM_LE: u32 = 0x001;
pub const TIOCM_DTR: u32 = 0x002;
pub const TIOCM_RTS: u32 = 0x004;
pub const TIOCM_ST: u32 = 0x008;
pub const TIOCM_SR: u32 = 0x010;
pub const TIOCM_CTS: u32 = 0x020;
pub const TIOCM_CAR: u32 = 0x040;
pub const TIOCM_RNG: u32 = 0x080;
pub const TIOCM_DSR: u32 = 0x100;

// ---------------------------------------------------------------------------
// Line discipline numbers (`TIOCSETD`)
// ---------------------------------------------------------------------------

pub const N_TTY: u32 = 0;
pub const N_SLIP: u32 = 1;
pub const N_MOUSE: u32 = 2;
pub const N_PPP: u32 = 3;
pub const N_STRIP: u32 = 4;
pub const N_AX25: u32 = 5;
pub const N_X25: u32 = 6;
pub const N_6PACK: u32 = 7;
pub const N_HCI: u32 = 15;

// ---------------------------------------------------------------------------
// Common terminal devices
// ---------------------------------------------------------------------------

pub const DEV_TTY: &str = "/dev/tty";
pub const DEV_CONSOLE: &str = "/dev/console";
pub const DEV_TTY0: &str = "/dev/tty0";
pub const DEV_TTYS0: &str = "/dev/ttyS0";

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub const TTY_DEFAULT_ROWS: u16 = 24;
pub const TTY_DEFAULT_COLS: u16 = 80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcgets_block_dense_0x5401_to_0x5408() {
        let g = [
            TCGETS, TCSETS, TCSETSW, TCSETSF, TCGETA, TCSETA, TCSETAW, TCSETAF,
        ];
        for (i, &v) in g.iter().enumerate() {
            assert_eq!(v, 0x5401 + i as u32);
        }
    }

    #[test]
    fn test_winsize_pair_adjacent() {
        // TIOCGWINSZ / TIOCSWINSZ sit on adjacent ioctl numbers.
        assert_eq!(TIOCSWINSZ, TIOCGWINSZ + 1);
        assert_eq!(TIOCGWINSZ, 0x5413);
    }

    #[test]
    fn test_modem_bits_dense_powers_of_two() {
        let m = [
            TIOCM_LE, TIOCM_DTR, TIOCM_RTS, TIOCM_ST, TIOCM_SR, TIOCM_CTS, TIOCM_CAR, TIOCM_RNG,
            TIOCM_DSR,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // OR of all 9 bits is exactly 0x1FF.
        let mut or = 0u32;
        for v in m {
            or |= v;
        }
        assert_eq!(or, 0x1FF);
    }

    #[test]
    fn test_line_discipline_numbers_dense_0_to_7() {
        let n = [N_TTY, N_SLIP, N_MOUSE, N_PPP, N_STRIP, N_AX25, N_X25, N_6PACK];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // HCI (Bluetooth) sat at 15 — leaving a gap for the experimental
        // disciplines kernel devs were trying out in 2.4.
        assert_eq!(N_HCI, 15);
    }

    #[test]
    fn test_tioc_block_in_0x54xx() {
        // All TIOC* are in the 0x54xx range (T = 0x54 = ASCII 'T').
        let i = [
            TIOCSCTTY, TIOCGPGRP, TIOCSPGRP, TIOCOUTQ, TIOCSTI, TIOCGWINSZ, TIOCSWINSZ, TIOCMGET,
            TIOCMSET, FIONREAD, TIOCEXCL, TIOCNXCL, TIOCNOTTY,
        ];
        for v in i {
            assert_eq!(v & 0xFF00, 0x5400);
        }
    }

    #[test]
    fn test_default_tty_size_is_vt100_80x24() {
        // The classic VT100 dimensions are the historical default when
        // no window-size message has arrived.
        assert_eq!(TTY_DEFAULT_COLS, 80);
        assert_eq!(TTY_DEFAULT_ROWS, 24);
        assert_eq!(DEV_TTY, "/dev/tty");
        assert_eq!(DEV_CONSOLE, "/dev/console");
    }
}
