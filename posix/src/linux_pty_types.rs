//! `<linux/pty.h>` — Pseudoterminal (PTY) constants.
//!
//! Pseudoterminals are pairs of virtual character devices: a master
//! (ptmx) and a slave (pts/N). The master side is held by a terminal
//! emulator or SSH daemon; the slave side looks like a real terminal
//! to the program running in it. Opening /dev/ptmx allocates a new
//! pair. PTYs provide job control, line editing, and signal
//! generation for programs that expect a terminal (shells, editors).

// ---------------------------------------------------------------------------
// PTY limits
// ---------------------------------------------------------------------------

/// Maximum number of PTY pairs (Unix98 style).
pub const NR_UNIX98_PTY_MAX: u32 = 4096;
/// Default maximum PTYs (can be changed via sysctl).
pub const NR_UNIX98_PTY_DEFAULT: u32 = 4096;
/// Maximum PTY index number.
pub const PTY_INDEX_MAX: u32 = 0x0FFF_FFFF;

// ---------------------------------------------------------------------------
// PTY ioctl commands
// ---------------------------------------------------------------------------

/// Get the PTY slave number (used after opening /dev/ptmx).
pub const TIOCGPTN: u32 = 0x8004_5430;
/// Lock/unlock PTY slave (must unlock before open).
pub const TIOCSPTLCK: u32 = 0x4004_5431;
/// Get PTY packet mode status.
pub const TIOCPKT: u32 = 0x5420;
/// Set PTY peer namespace.
pub const TIOCGPTPEER: u32 = 0x5441;

// ---------------------------------------------------------------------------
// PTY packet mode flags (TIOCPKT)
// ---------------------------------------------------------------------------

/// Data packet (normal data follows).
pub const TIOCPKT_DATA: u32 = 0x00;
/// Flush read queue.
pub const TIOCPKT_FLUSHREAD: u32 = 0x01;
/// Flush write queue.
pub const TIOCPKT_FLUSHWRITE: u32 = 0x02;
/// Stop output (XOFF sent).
pub const TIOCPKT_STOP: u32 = 0x04;
/// Start output (XON sent).
pub const TIOCPKT_START: u32 = 0x08;
/// Output was stopped (flow control).
pub const TIOCPKT_NOSTOP: u32 = 0x10;
/// Output started (flow control resumed).
pub const TIOCPKT_DOSTOP: u32 = 0x20;
/// Window size changed (SIGWINCH).
pub const TIOCPKT_IOCTL: u32 = 0x40;

// ---------------------------------------------------------------------------
// PTY master open flags
// ---------------------------------------------------------------------------

/// Open PTY master non-blocking.
pub const PTMX_O_NONBLOCK: u32 = 0o4000;
/// Open PTY master close-on-exec.
pub const PTMX_O_CLOEXEC: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_limits() {
        assert!(NR_UNIX98_PTY_MAX > 0);
        assert!(PTY_INDEX_MAX > NR_UNIX98_PTY_DEFAULT);
    }

    #[test]
    fn test_packet_flags_no_overlap() {
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
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_data_is_zero() {
        assert_eq!(TIOCPKT_DATA, 0);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [TIOCGPTN, TIOCSPTLCK, TIOCPKT, TIOCGPTPEER];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
