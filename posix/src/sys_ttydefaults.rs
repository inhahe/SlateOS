//! `<sys/ttydefaults.h>` — default terminal control characters.
//!
//! Provides the default values for terminal control characters
//! (used when initializing a new terminal or resetting to defaults).

/// Default ERASE character (DEL).
pub const CERASE: u8 = 0x7F;

/// Default KILL character (Ctrl-U).
pub const CKILL: u8 = 0x15; // ^U

/// Default INTR character (Ctrl-C).
pub const CINTR: u8 = 0x03; // ^C

/// Default QUIT character (Ctrl-\\).
pub const CQUIT: u8 = 0x1C; // ^\

/// Default START character (Ctrl-Q).
pub const CSTART: u8 = 0x11; // ^Q

/// Default STOP character (Ctrl-S).
pub const CSTOP: u8 = 0x13; // ^S

/// Default SUSP character (Ctrl-Z).
pub const CSUSP: u8 = 0x1A; // ^Z

/// Default EOF character (Ctrl-D).
pub const CEOF: u8 = 0x04; // ^D

/// Default EOL character (NUL — disabled).
pub const CEOL: u8 = 0;

/// Default REPRINT character (Ctrl-R).
pub const CREPRINT: u8 = 0x12; // ^R

/// Default DISCARD character (Ctrl-O).
pub const CDISCARD: u8 = 0x0F; // ^O

/// Default WERASE character (Ctrl-W).
pub const CWERASE: u8 = 0x17; // ^W

/// Default LNEXT character (Ctrl-V).
pub const CLNEXT: u8 = 0x16; // ^V

/// Default MIN value (1 character).
pub const CMIN: u8 = 1;

/// Default TIME value (0 — no timeout).
pub const CTIME: u8 = 0;

/// Default terminal input speed (B9600).
pub const TTYDEF_SPEED: u32 = crate::termios::B9600;

/// Default input flags.
pub const TTYDEF_IFLAG: u32 = crate::termios::BRKINT | crate::termios::ICRNL | crate::termios::IXON;

/// Default output flags.
pub const TTYDEF_OFLAG: u32 = crate::termios::OPOST | crate::termios::ONLCR;

/// Default local flags.
pub const TTYDEF_LFLAG: u32 = crate::termios::ECHO
    | crate::termios::ICANON
    | crate::termios::ISIG
    | crate::termios::IEXTEN
    | crate::termios::ECHOE;

/// Default control flags.
pub const TTYDEF_CFLAG: u32 = crate::termios::CS8 | crate::termios::CREAD | crate::termios::HUPCL;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_chars_distinct() {
        let chars = [
            CINTR, CQUIT, CERASE, CKILL, CEOF, CSTART, CSTOP, CSUSP, CREPRINT, CDISCARD, CWERASE,
            CLNEXT,
        ];
        for i in 0..chars.len() {
            for j in (i + 1)..chars.len() {
                assert_ne!(chars[i], chars[j], "default cc values must be distinct");
            }
        }
    }

    #[test]
    fn test_cerase_is_del() {
        assert_eq!(CERASE, 127);
    }

    #[test]
    fn test_cintr_is_ctrl_c() {
        assert_eq!(CINTR, 3);
    }

    #[test]
    fn test_ceof_is_ctrl_d() {
        assert_eq!(CEOF, 4);
    }

    #[test]
    fn test_csusp_is_ctrl_z() {
        assert_eq!(CSUSP, 26);
    }

    #[test]
    fn test_ttydef_speed() {
        assert_eq!(TTYDEF_SPEED, crate::termios::B9600);
    }

    #[test]
    fn test_ttydef_flags_nonzero() {
        assert_ne!(TTYDEF_IFLAG, 0);
        assert_ne!(TTYDEF_OFLAG, 0);
        assert_ne!(TTYDEF_LFLAG, 0);
        assert_ne!(TTYDEF_CFLAG, 0);
    }

    #[test]
    fn test_cmin_ctime() {
        assert_eq!(CMIN, 1);
        assert_eq!(CTIME, 0);
    }
}
