//! `<asm-generic/ioctls.h>` — Terminal window size and ioctl constants.
//!
//! These constants define the ioctls for getting and setting
//! terminal window size (`struct winsize`) and other commonly
//! used terminal control operations.

// ---------------------------------------------------------------------------
// Window size ioctls
// ---------------------------------------------------------------------------

/// Get terminal window size.
pub const TIOCGWINSZ: u32 = 0x5413;
/// Set terminal window size.
pub const TIOCSWINSZ: u32 = 0x5414;

// ---------------------------------------------------------------------------
// Process group / session ioctls
// ---------------------------------------------------------------------------

/// Get foreground process group.
pub const TIOCGPGRP: u32 = 0x540F;
/// Set foreground process group.
pub const TIOCSPGRP: u32 = 0x5410;
/// Get session ID.
pub const TIOCGSID: u32 = 0x5429;

// ---------------------------------------------------------------------------
// Exclusive mode ioctls
// ---------------------------------------------------------------------------

/// Set exclusive mode.
pub const TIOCEXCL: u32 = 0x540C;
/// Clear exclusive mode.
pub const TIOCNXCL: u32 = 0x540D;
/// Get exclusive mode.
pub const TIOCGEXCL: u32 = 0x5440;

// ---------------------------------------------------------------------------
// TTY ioctl commands
// ---------------------------------------------------------------------------

/// Set controlling terminal.
pub const TIOCSCTTY: u32 = 0x540E;
/// Give up controlling terminal.
pub const TIOCNOTTY: u32 = 0x5422;
/// Get number of bytes available to read.
pub const FIONREAD: u32 = 0x541B;
/// Set/clear non-blocking I/O.
pub const FIONBIO: u32 = 0x5421;
/// Drain output (wait for write completion).
pub const TCSBRK_DRAIN: u32 = 0x5409;

// ---------------------------------------------------------------------------
// Pseudo-terminal ioctls
// ---------------------------------------------------------------------------

/// Get PTY number.
pub const TIOCGPTN: u32 = 0x8004_5430;
/// Lock/unlock PTY.
pub const TIOCSPTLCK: u32 = 0x4004_5431;
/// Get PTY lock status.
pub const TIOCGPTLCK: u32 = 0x8004_5439;
/// Set packet mode.
pub const TIOCPKT: u32 = 0x5420;
/// Get packet mode.
pub const TIOCGPKT: u32 = 0x8004_5438;

// ---------------------------------------------------------------------------
// Packet mode flags (TIOCPKT data byte)
// ---------------------------------------------------------------------------

/// Data packet.
pub const TIOCPKT_DATA: u8 = 0x00;
/// Flush read.
pub const TIOCPKT_FLUSHREAD: u8 = 0x01;
/// Flush write.
pub const TIOCPKT_FLUSHWRITE: u8 = 0x02;
/// Stop output.
pub const TIOCPKT_STOP: u8 = 0x04;
/// Start output.
pub const TIOCPKT_START: u8 = 0x08;
/// No stop character.
pub const TIOCPKT_NOSTOP: u8 = 0x10;
/// Do stop character.
pub const TIOCPKT_DOSTOP: u8 = 0x20;
/// I/O control changed.
pub const TIOCPKT_IOCTL: u8 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winsz_ioctls() {
        assert_eq!(TIOCGWINSZ, 0x5413);
        assert_eq!(TIOCSWINSZ, 0x5414);
        assert_ne!(TIOCGWINSZ, TIOCSWINSZ);
    }

    #[test]
    fn test_pgrp_ioctls_distinct() {
        let ioctls = [TIOCGPGRP, TIOCSPGRP, TIOCGSID];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_excl_ioctls_distinct() {
        let ioctls = [TIOCEXCL, TIOCNXCL, TIOCGEXCL];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_pty_ioctls_distinct() {
        let ioctls = [TIOCGPTN, TIOCSPTLCK, TIOCGPTLCK, TIOCPKT, TIOCGPKT];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_pkt_flags_no_overlap() {
        let flags = [
            TIOCPKT_FLUSHREAD,
            TIOCPKT_FLUSHWRITE,
            TIOCPKT_STOP,
            TIOCPKT_START,
            TIOCPKT_NOSTOP,
            TIOCPKT_DOSTOP,
            TIOCPKT_IOCTL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pkt_data_is_zero() {
        assert_eq!(TIOCPKT_DATA, 0);
    }

    #[test]
    fn test_fionread() {
        assert_eq!(FIONREAD, 0x541B);
    }
}
