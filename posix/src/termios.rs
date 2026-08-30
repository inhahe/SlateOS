//! `<termios.h>` — terminal I/O control.
//!
//! Re-exports the `Termios` structure, control constants, and terminal
//! manipulation functions from the `ioctl` module.  Programs that
//! include `<termios.h>` can find everything here.

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

pub use crate::ioctl::Termios;

// ---------------------------------------------------------------------------
// Action constants for tcsetattr
// ---------------------------------------------------------------------------

pub use crate::ioctl::TCSADRAIN;
pub use crate::ioctl::TCSAFLUSH;
pub use crate::ioctl::TCSANOW;

// ---------------------------------------------------------------------------
// Input mode flags (c_iflag)
// ---------------------------------------------------------------------------

pub use crate::ioctl::BRKINT;
pub use crate::ioctl::ICRNL;
pub use crate::ioctl::IGNCR;
pub use crate::ioctl::INLCR;
pub use crate::ioctl::INPCK;
pub use crate::ioctl::ISTRIP;
pub use crate::ioctl::IXON;

/// Ignore BREAK condition.
pub const IGNBRK: u32 = 0o0001;

/// Ignore characters with parity errors.
pub const IGNPAR: u32 = 0o0004;

/// Mark parity or framing errors.
pub const PARMRK: u32 = 0o0010;

/// Enable XON/XOFF flow control on output.
pub const IXOFF: u32 = 0o10000;

/// Any character will restart after stop.
pub const IXANY: u32 = 0o4000;

/// Ring bell when input queue is full.
pub const IMAXBEL: u32 = 0o20000;

/// UTF-8 input processing.
pub const IUTF8: u32 = 0o40000;

// ---------------------------------------------------------------------------
// Output mode flags (c_oflag)
// ---------------------------------------------------------------------------

pub use crate::ioctl::ONLCR;
pub use crate::ioctl::OPOST;

/// Map CR to NL on output.
pub const OCRNL: u32 = 0o10;

/// No CR output at column 0.
pub const ONOCR: u32 = 0o20;

/// NL performs CR function.
pub const ONLRET: u32 = 0o40;

/// Send fill characters for a delay.
pub const OFILL: u32 = 0o100;

/// Fill character is DEL (0x7F); otherwise NUL.
pub const OFDEL: u32 = 0o200;

// ---------------------------------------------------------------------------
// Control mode flags (c_cflag)
// ---------------------------------------------------------------------------

pub use crate::ioctl::CLOCAL;
pub use crate::ioctl::CREAD;
pub use crate::ioctl::CS8;
pub use crate::ioctl::CSIZE;
pub use crate::ioctl::HUPCL;
pub use crate::ioctl::PARENB;

/// 5-bit characters.
pub const CS5: u32 = 0o0;

/// 6-bit characters.
pub const CS6: u32 = 0o20;

/// 7-bit characters.
pub const CS7: u32 = 0o40;

/// Two stop bits (else one).
pub const CSTOPB: u32 = 0o100;

/// Odd parity (else even).
pub const PARODD: u32 = 0o1000;

// ---------------------------------------------------------------------------
// Local mode flags (c_lflag)
// ---------------------------------------------------------------------------

pub use crate::ioctl::ECHO;
pub use crate::ioctl::ECHONL;
pub use crate::ioctl::ICANON;
pub use crate::ioctl::IEXTEN;
pub use crate::ioctl::ISIG;

/// Echo erase character as BS-SP-BS.
pub const ECHOE: u32 = 0o20;

/// Echo NL after kill character.
pub const ECHOK: u32 = 0o40;

/// Enable implementation-defined input processing.
pub const NOFLSH: u32 = 0o200;

/// Send SIGTTOU for background output.
pub const TOSTOP: u32 = 0o400;

// ---------------------------------------------------------------------------
// Control character indices
// ---------------------------------------------------------------------------

pub use crate::ioctl::NCCS;
pub use crate::ioctl::VEOF;
pub use crate::ioctl::VEOL;
pub use crate::ioctl::VERASE;
pub use crate::ioctl::VINTR;
pub use crate::ioctl::VKILL;
pub use crate::ioctl::VMIN;
pub use crate::ioctl::VQUIT;
pub use crate::ioctl::VSTART;
pub use crate::ioctl::VSTOP;
pub use crate::ioctl::VSUSP;
pub use crate::ioctl::VTIME;

/// Second EOL character index.
pub const VEOL2: usize = 16;

/// Literal-next character index.
pub const VLNEXT: usize = 15;

/// Word-erase character index.
pub const VWERASE: usize = 14;

/// Reprint-line character index.
pub const VREPRINT: usize = 12;

/// Discard-output character index.
pub const VDISCARD: usize = 13;

// ---------------------------------------------------------------------------
// Baud rates
// ---------------------------------------------------------------------------

pub use crate::ioctl::B9600;
pub use crate::ioctl::B19200;
pub use crate::ioctl::B38400;
pub use crate::ioctl::B115200;

/// Hang up (0 baud).
pub const B0: u32 = 0o0;

/// 50 baud.
pub const B50: u32 = 0o1;

/// 75 baud.
pub const B75: u32 = 0o2;

/// 110 baud.
pub const B110: u32 = 0o3;

/// 134 baud.
pub const B134: u32 = 0o4;

/// 150 baud.
pub const B150: u32 = 0o5;

/// 200 baud.
pub const B200: u32 = 0o6;

/// 300 baud.
pub const B300: u32 = 0o7;

/// 600 baud.
pub const B600: u32 = 0o10;

/// 1200 baud.
pub const B1200: u32 = 0o11;

/// 1800 baud.
pub const B1800: u32 = 0o12;

/// 2400 baud.
pub const B2400: u32 = 0o13;

/// 4800 baud.
pub const B4800: u32 = 0o14;

/// 57600 baud.
pub const B57600: u32 = 0o10001;

/// 230400 baud.
pub const B230400: u32 = 0o10003;

/// 460800 baud.
pub const B460800: u32 = 0o10004;

// ---------------------------------------------------------------------------
// Flow control actions (tcflow)
// ---------------------------------------------------------------------------

pub use crate::ioctl::TCIOFF;
pub use crate::ioctl::TCION;
pub use crate::ioctl::TCOOFF;
pub use crate::ioctl::TCOON;

// ---------------------------------------------------------------------------
// Queue selectors (tcflush)
// ---------------------------------------------------------------------------

pub use crate::ioctl::TCIFLUSH;
pub use crate::ioctl::TCIOFLUSH;
pub use crate::ioctl::TCOFLUSH;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::ioctl::cfmakeraw;
pub use crate::ioctl::cfsetspeed;
pub use crate::ioctl::tcdrain;
pub use crate::ioctl::tcflow;
pub use crate::ioctl::tcflush;
pub use crate::ioctl::tcgetattr;
pub use crate::ioctl::tcsendbreak;
pub use crate::ioctl::tcsetattr;

// ---------------------------------------------------------------------------
// Speed query/set (re-exports from ioctl)
// ---------------------------------------------------------------------------

pub use crate::ioctl::cfgetispeed;
pub use crate::ioctl::cfgetospeed;
pub use crate::ioctl::cfsetispeed;
pub use crate::ioctl::cfsetospeed;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_termios_struct_size() {
        assert!(core::mem::size_of::<Termios>() > 0);
    }

    #[test]
    fn test_tcsanow_values() {
        assert_eq!(TCSANOW, 0);
        assert_eq!(TCSADRAIN, 1);
        assert_eq!(TCSAFLUSH, 2);
    }

    #[test]
    fn test_cs_flags() {
        assert_eq!(CS5, 0);
        assert!(CS6 > CS5);
        assert!(CS7 > CS6);
    }

    #[test]
    fn test_baud_rates_ascending() {
        assert_eq!(B0, 0);
        assert!(B50 > B0);
        assert!(B75 > B50);
        assert!(B110 > B75);
        assert!(B300 > B200);
        assert!(B9600 > B4800);
    }

    #[test]
    fn test_cc_indices_distinct() {
        let indices = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME, VMIN, VSTART, VSTOP, VSUSP, VEOL, VREPRINT,
            VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j], "cc indices must be distinct");
            }
        }
    }

    #[test]
    fn test_cc_indices_in_range() {
        let indices = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME, VMIN, VSTART, VSTOP, VSUSP, VEOL, VREPRINT,
            VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for &idx in &indices {
            assert!(idx < NCCS, "cc index {idx} must be < NCCS ({NCCS})");
        }
    }

    #[test]
    fn test_cfgetispeed_null() {
        assert_eq!(unsafe { cfgetispeed(core::ptr::null()) }, 0);
    }

    #[test]
    fn test_cfsetispeed_null() {
        assert_eq!(unsafe { cfsetispeed(core::ptr::null_mut(), B9600) }, -1);
    }

    #[test]
    fn test_cfset_cfget_roundtrip() {
        let mut t = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0u8; NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        unsafe {
            assert_eq!(cfsetispeed(&mut t, B9600), 0);
            assert_eq!(cfgetispeed(&t), B9600);
        }
    }

    #[test]
    fn test_cfsetospeed_roundtrip() {
        let mut t = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0u8; NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        unsafe {
            assert_eq!(cfsetospeed(&mut t, B115200), 0);
            assert_eq!(cfgetospeed(&t), B115200);
        }
    }

    #[test]
    fn test_input_flags_nonzero() {
        assert_ne!(BRKINT, 0);
        assert_ne!(ICRNL, 0);
        assert_ne!(IXON, 0);
        assert_ne!(IGNBRK, 0);
        assert_ne!(IXOFF, 0);
        assert_ne!(IUTF8, 0);
    }

    #[test]
    fn test_output_flags_nonzero() {
        assert_ne!(OPOST, 0);
        assert_ne!(ONLCR, 0);
        assert_ne!(OCRNL, 0);
    }

    #[test]
    fn test_local_flags_nonzero() {
        assert_ne!(ISIG, 0);
        assert_ne!(ICANON, 0);
        assert_ne!(ECHO, 0);
        assert_ne!(ECHOE, 0);
        assert_ne!(ECHOK, 0);
        assert_ne!(TOSTOP, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(TCSANOW, crate::ioctl::TCSANOW);
        assert_eq!(B9600, crate::ioctl::B9600);
        assert_eq!(ECHO, crate::ioctl::ECHO);
        assert_eq!(NCCS, crate::ioctl::NCCS);
    }
}
