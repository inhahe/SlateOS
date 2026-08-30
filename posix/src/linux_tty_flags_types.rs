//! `<asm-generic/termbits.h>` — Terminal I/O flag constants.
//!
//! These bitmask constants control terminal behavior: input
//! processing (c_iflag), output processing (c_oflag), control
//! modes (c_cflag), and local modes (c_lflag) in `struct termios`.

// ---------------------------------------------------------------------------
// Input flags (c_iflag)
// ---------------------------------------------------------------------------

/// Ignore break condition.
pub const IGNBRK: u32 = 0o000001;
/// Signal interrupt on break.
pub const BRKINT: u32 = 0o000002;
/// Ignore characters with parity errors.
pub const IGNPAR: u32 = 0o000004;
/// Mark parity and framing errors.
pub const PARMRK: u32 = 0o000010;
/// Enable input parity checking.
pub const INPCK: u32 = 0o000020;
/// Strip eighth bit.
pub const ISTRIP: u32 = 0o000040;
/// Map NL to CR on input.
pub const INLCR: u32 = 0o000100;
/// Ignore CR on input.
pub const IGNCR: u32 = 0o000200;
/// Map CR to NL on input.
pub const ICRNL: u32 = 0o000400;
/// Enable start/stop output control.
pub const IXON: u32 = 0o002000;
/// Enable start/stop input control.
pub const IXOFF: u32 = 0o010000;
/// Enable any character to restart output.
pub const IXANY: u32 = 0o004000;
/// Ring bell when input queue is full.
pub const IMAXBEL: u32 = 0o020000;
/// Input is UTF-8.
pub const IUTF8: u32 = 0o040000;

// ---------------------------------------------------------------------------
// Output flags (c_oflag)
// ---------------------------------------------------------------------------

/// Post-process output.
pub const OPOST: u32 = 0o000001;
/// Map NL to CR-NL on output.
pub const ONLCR: u32 = 0o000004;
/// Map CR to NL on output.
pub const OCRNL: u32 = 0o000010;
/// No CR output at column 0.
pub const ONOCR: u32 = 0o000020;
/// No CR in output.
pub const ONLRET: u32 = 0o000040;

// ---------------------------------------------------------------------------
// Local flags (c_lflag)
// ---------------------------------------------------------------------------

/// Enable signals (INTR, QUIT, SUSP).
pub const ISIG: u32 = 0o000001;
/// Canonical mode (line editing).
pub const ICANON: u32 = 0o000002;
/// Echo input characters.
pub const ECHO: u32 = 0o000010;
/// Echo erase as backspace-space-backspace.
pub const ECHOE: u32 = 0o000020;
/// Echo kill by erasing line.
pub const ECHOK: u32 = 0o000040;
/// Echo NL even if ECHO is off.
pub const ECHONL: u32 = 0o000100;
/// Disable flush after interrupt/quit/suspend.
pub const NOFLSH: u32 = 0o000200;
/// Send SIGTTOU for background writes.
pub const TOSTOP: u32 = 0o000400;
/// Enable implementation-defined processing.
pub const IEXTEN: u32 = 0o100000;

// ---------------------------------------------------------------------------
// Control flags (c_cflag)
// ---------------------------------------------------------------------------

/// Character size mask.
pub const CSIZE: u32 = 0o000060;
/// 5-bit characters.
pub const CS5: u32 = 0o000000;
/// 6-bit characters.
pub const CS6: u32 = 0o000020;
/// 7-bit characters.
pub const CS7: u32 = 0o000040;
/// 8-bit characters.
pub const CS8: u32 = 0o000060;
/// Two stop bits.
pub const CSTOPB: u32 = 0o000100;
/// Enable receiver.
pub const CREAD: u32 = 0o000200;
/// Enable parity.
pub const PARENB: u32 = 0o000400;
/// Odd parity (even if not set).
pub const PARODD: u32 = 0o001000;
/// Hang up on last close.
pub const HUPCL: u32 = 0o002000;
/// Ignore modem status lines.
pub const CLOCAL: u32 = 0o004000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iflags_values() {
        assert_eq!(IGNBRK, 1);
        assert_eq!(ICRNL, 0o000400);
    }

    #[test]
    fn test_oflags_values() {
        assert_eq!(OPOST, 1);
    }

    #[test]
    fn test_lflags_values() {
        assert_eq!(ISIG, 1);
        assert_eq!(ICANON, 2);
        assert_eq!(ECHO, 8);
    }

    #[test]
    fn test_csize_mask() {
        assert_eq!(CSIZE, 0o000060);
        assert_eq!(CS5 & CSIZE, CS5);
        assert_eq!(CS8 & CSIZE, CS8);
    }

    #[test]
    fn test_char_sizes_within_mask() {
        assert_eq!(CS5 & !CSIZE, 0);
        assert_eq!(CS6 & !CSIZE, 0);
        assert_eq!(CS7 & !CSIZE, 0);
        assert_eq!(CS8 & !CSIZE, 0);
    }

    #[test]
    fn test_char_sizes_distinct() {
        let sizes = [CS5, CS6, CS7, CS8];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_iutf8() {
        assert_eq!(IUTF8, 0o040000);
    }
}
