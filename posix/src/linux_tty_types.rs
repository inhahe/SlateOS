//! `<linux/tty.h>` / `<termios.h>` — TTY and termios constants.
//!
//! The TTY subsystem provides terminal I/O with line discipline
//! processing (echo, line editing, signal generation). termios
//! controls input/output modes, baud rates, and control characters.
//! Used by shells, terminal emulators, and serial communications.

// ---------------------------------------------------------------------------
// termios c_iflag (input modes)
// ---------------------------------------------------------------------------

/// Ignore BREAK condition.
pub const IGNBRK: u32 = 0o000001;
/// Signal BREAK.
pub const BRKINT: u32 = 0o000002;
/// Ignore parity errors.
pub const IGNPAR: u32 = 0o000004;
/// Mark parity errors.
pub const PARMRK: u32 = 0o000010;
/// Enable input parity checking.
pub const INPCK: u32 = 0o000020;
/// Strip 8th bit.
pub const ISTRIP: u32 = 0o000040;
/// Map NL to CR on input.
pub const INLCR: u32 = 0o000100;
/// Ignore CR on input.
pub const IGNCR: u32 = 0o000200;
/// Map CR to NL on input.
pub const ICRNL: u32 = 0o000400;
/// Enable XON/XOFF flow control on output.
pub const IXON: u32 = 0o002000;
/// Enable XON/XOFF flow control on input.
pub const IXOFF: u32 = 0o010000;

// ---------------------------------------------------------------------------
// termios c_oflag (output modes)
// ---------------------------------------------------------------------------

/// Enable output processing.
pub const OPOST: u32 = 0o000001;
/// Map NL to CR-NL on output.
pub const ONLCR: u32 = 0o000004;

// ---------------------------------------------------------------------------
// termios c_cflag (control modes)
// ---------------------------------------------------------------------------

/// Character size mask.
pub const CSIZE: u32 = 0o000060;
/// 5 bits per character.
pub const CS5: u32 = 0o000000;
/// 6 bits per character.
pub const CS6: u32 = 0o000020;
/// 7 bits per character.
pub const CS7: u32 = 0o000040;
/// 8 bits per character.
pub const CS8: u32 = 0o000060;
/// Two stop bits.
pub const CSTOPB: u32 = 0o000100;
/// Enable receiver.
pub const CREAD: u32 = 0o000200;
/// Enable parity.
pub const PARENB: u32 = 0o000400;
/// Odd parity.
pub const PARODD: u32 = 0o001000;
/// Hang up on last close.
pub const HUPCL: u32 = 0o002000;
/// Ignore modem control lines.
pub const CLOCAL: u32 = 0o004000;

// ---------------------------------------------------------------------------
// termios c_lflag (local modes)
// ---------------------------------------------------------------------------

/// Enable signals (SIGINT, SIGQUIT, SIGSUSP).
pub const ISIG: u32 = 0o000001;
/// Canonical mode (line-by-line input).
pub const ICANON: u32 = 0o000002;
/// Echo input characters.
pub const ECHO: u32 = 0o000010;
/// Echo erase as BS-SP-BS.
pub const ECHOE: u32 = 0o000020;
/// Echo NL even if ECHO is off.
pub const ECHONL: u32 = 0o000100;
/// Disable flush after interrupt.
pub const NOFLSH: u32 = 0o000200;
/// Send SIGTTOU for background output.
pub const TOSTOP: u32 = 0o000400;

// ---------------------------------------------------------------------------
// tcsetattr when argument
// ---------------------------------------------------------------------------

/// Change immediately.
pub const TCSANOW: u32 = 0;
/// Change after output drains.
pub const TCSADRAIN: u32 = 1;
/// Change after output drains, flush input.
pub const TCSAFLUSH: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iflag_distinct() {
        let flags = [
            IGNBRK, BRKINT, IGNPAR, PARMRK, INPCK, ISTRIP, INLCR, IGNCR, ICRNL, IXON, IXOFF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_lflag_distinct() {
        let flags = [ISIG, ICANON, ECHO, ECHOE, ECHONL, NOFLSH, TOSTOP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_cs8_in_csize() {
        assert_eq!(CS8 & CSIZE, CS8);
    }

    #[test]
    fn test_tcsetattr_distinct() {
        assert_ne!(TCSANOW, TCSADRAIN);
        assert_ne!(TCSADRAIN, TCSAFLUSH);
        assert_ne!(TCSANOW, TCSAFLUSH);
    }
}
