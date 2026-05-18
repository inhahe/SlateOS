//! `<unistd.h>` — Session and controlling terminal constants.
//!
//! Sessions group processes for job control.  A session leader
//! creates a new session with `setsid()` and can acquire a
//! controlling terminal.  These constants define related
//! values and ioctl commands.

// ---------------------------------------------------------------------------
// Session/process group IDs
// ---------------------------------------------------------------------------

/// No session leader (invalid session ID).
pub const SID_NONE: i32 = -1;

// ---------------------------------------------------------------------------
// Controlling terminal ioctls
// ---------------------------------------------------------------------------

/// Set the controlling terminal (ioctl TIOCSCTTY).
pub const TIOCSCTTY: u32 = 0x540E;
/// Give up the controlling terminal (ioctl TIOCNOTTY).
pub const TIOCNOTTY: u32 = 0x5422;
/// Get the session ID of the terminal (ioctl TIOCGSID).
pub const TIOCGSID: u32 = 0x5429;
/// Get the foreground process group (ioctl TIOCGPGRP).
pub const TIOCGPGRP: u32 = 0x540F;
/// Set the foreground process group (ioctl TIOCSPGRP).
pub const TIOCSPGRP: u32 = 0x5410;

// ---------------------------------------------------------------------------
// tcsetpgrp/tcgetpgrp (POSIX wrappers around TIOCSPGRP/TIOCGPGRP)
// ---------------------------------------------------------------------------

/// Error return from tcgetpgrp().
pub const TCGETPGRP_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// setsid() error
// ---------------------------------------------------------------------------

/// Error return from setsid() (already a session leader).
pub const SETSID_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Job control signals (relevant to sessions)
// ---------------------------------------------------------------------------

/// Signal sent to foreground process group on terminal hangup.
pub const SESSION_SIGHUP: u32 = 1;
/// Signal sent to background processes trying to read terminal.
pub const SESSION_SIGTTIN: u32 = 21;
/// Signal sent to background processes trying to write terminal.
pub const SESSION_SIGTTOU: u32 = 22;
/// Signal sent to foreground process group on suspend (Ctrl+Z).
pub const SESSION_SIGTSTP: u32 = 20;
/// Signal to continue a stopped process.
pub const SESSION_SIGCONT: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sid_none() {
        assert_eq!(SID_NONE, -1);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            TIOCSCTTY, TIOCNOTTY, TIOCGSID,
            TIOCGPGRP, TIOCSPGRP,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_tiocsctty_value() {
        assert_eq!(TIOCSCTTY, 0x540E);
    }

    #[test]
    fn test_tiocnotty_value() {
        assert_eq!(TIOCNOTTY, 0x5422);
    }

    #[test]
    fn test_error_returns() {
        assert_eq!(TCGETPGRP_ERROR, -1);
        assert_eq!(SETSID_ERROR, -1);
    }

    #[test]
    fn test_signals_distinct() {
        let sigs = [
            SESSION_SIGHUP, SESSION_SIGTTIN, SESSION_SIGTTOU,
            SESSION_SIGTSTP, SESSION_SIGCONT,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_sighup_is_one() {
        assert_eq!(SESSION_SIGHUP, 1);
    }
}
