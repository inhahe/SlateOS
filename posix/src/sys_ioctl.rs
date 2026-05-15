//! `<sys/ioctl.h>` — device control operations.
//!
//! Re-exports `ioctl()`, ioctl request codes, `Winsize`, and
//! `Termios` from the `ioctl` module.

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::ioctl::ioctl;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::ioctl::Winsize;
pub use crate::ioctl::Termios;

// ---------------------------------------------------------------------------
// Request codes
// ---------------------------------------------------------------------------

pub use crate::ioctl::TIOCGWINSZ;
pub use crate::ioctl::TIOCSWINSZ;
pub use crate::ioctl::FIONBIO;
pub use crate::ioctl::FIONREAD;
pub use crate::ioctl::TCGETS;
pub use crate::ioctl::TCSETS;
pub use crate::ioctl::TCSETSW;
pub use crate::ioctl::TCSETSF;
pub use crate::ioctl::TIOCSCTTY;
pub use crate::ioctl::TIOCGPGRP;
pub use crate::ioctl::TIOCSPGRP;
pub use crate::ioctl::TIOCNOTTY;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winsize_struct_size() {
        assert_eq!(core::mem::size_of::<Winsize>(), 8);
    }

    #[test]
    fn test_termios_struct_size() {
        assert!(core::mem::size_of::<Termios>() > 0);
    }

    #[test]
    fn test_ioctl_codes_distinct() {
        let codes: [u64; 8] = [
            TIOCGWINSZ, TIOCSWINSZ, FIONBIO, FIONREAD,
            TCGETS, TCSETS, TCSETSW, TCSETSF,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "ioctl codes must be distinct");
            }
        }
    }

    #[test]
    fn test_tioc_codes_distinct() {
        let codes: [u64; 4] = [
            TIOCSCTTY, TIOCGPGRP, TIOCSPGRP, TIOCNOTTY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_bad_fd() {
        let ret = ioctl(-1, TIOCGWINSZ, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(TIOCGWINSZ, crate::ioctl::TIOCGWINSZ);
        assert_eq!(FIONBIO, crate::ioctl::FIONBIO);
    }
}
