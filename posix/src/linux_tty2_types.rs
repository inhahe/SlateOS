//! `<linux/tty.h>` — Additional TTY constants.
//!
//! Supplementary TTY constants covering line disciplines,
//! TTY flags, N_TTY modes, and packet mode flags.

// ---------------------------------------------------------------------------
// TTY line disciplines (N_*)
// ---------------------------------------------------------------------------

/// Normal TTY.
pub const N_TTY: u32 = 0;
/// SLIP.
pub const N_SLIP: u32 = 1;
/// Mouse.
pub const N_MOUSE: u32 = 2;
/// PPP.
pub const N_PPP: u32 = 3;
/// Strip.
pub const N_STRIP: u32 = 4;
/// AX.25.
pub const N_AX25: u32 = 5;
/// X.25.
pub const N_X25: u32 = 6;
/// 6PACK.
pub const N_6PACK: u32 = 7;
/// IRDA.
pub const N_IRDA: u32 = 11;
/// HCI (Bluetooth).
pub const N_HCI: u32 = 15;
/// GSM 0710 mux.
pub const N_GSM0710: u32 = 21;
/// NCI (NFC).
pub const N_NCI: u32 = 25;

// ---------------------------------------------------------------------------
// TTY ioctl commands
// ---------------------------------------------------------------------------

/// Get line discipline.
pub const TIOCGETD: u32 = 0x5424;
/// Set line discipline.
pub const TIOCSETD: u32 = 0x5423;
/// Non-blocking.
pub const TIOCNXCL: u32 = 0x540D;
/// Exclusive.
pub const TIOCSCTTY: u32 = 0x540E;
/// Get process group.
pub const TIOCGPGRP: u32 = 0x540F;
/// Set process group.
pub const TIOCSPGRP: u32 = 0x5410;
/// Get window size.
pub const TIOCGWINSZ: u32 = 0x5413;
/// Set window size.
pub const TIOCSWINSZ: u32 = 0x5414;
/// Get modem status.
pub const TIOCMGET: u32 = 0x5415;
/// Set modem bits.
pub const TIOCMBIS: u32 = 0x5416;
/// Clear modem bits.
pub const TIOCMBIC: u32 = 0x5417;
/// Set modem status.
pub const TIOCMSET: u32 = 0x5418;
/// Drain output.
pub const TIOCDRAIN: u32 = 0x5419;
/// Set break.
pub const TIOCSBRK: u32 = 0x5427;
/// Clear break.
pub const TIOCCBRK: u32 = 0x5428;

// ---------------------------------------------------------------------------
// TTY packet mode flags (TIOCPKT_*)
// ---------------------------------------------------------------------------

/// Data follows.
pub const TIOCPKT_DATA: u8 = 0;
/// Flush read.
pub const TIOCPKT_FLUSHREAD: u8 = 1;
/// Flush write.
pub const TIOCPKT_FLUSHWRITE: u8 = 2;
/// Stop output.
pub const TIOCPKT_STOP: u8 = 4;
/// Start output.
pub const TIOCPKT_START: u8 = 8;
/// No stop.
pub const TIOCPKT_NOSTOP: u8 = 16;
/// Do stop.
pub const TIOCPKT_DOSTOP: u8 = 32;
/// I/O control data.
pub const TIOCPKT_IOCTL: u8 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_disciplines_distinct() {
        let discs = [
            N_TTY, N_SLIP, N_MOUSE, N_PPP, N_STRIP, N_AX25, N_X25, N_6PACK, N_IRDA, N_HCI,
            N_GSM0710, N_NCI,
        ];
        for i in 0..discs.len() {
            for j in (i + 1)..discs.len() {
                assert_ne!(discs[i], discs[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            TIOCGETD, TIOCSETD, TIOCNXCL, TIOCSCTTY, TIOCGPGRP, TIOCSPGRP, TIOCGWINSZ, TIOCSWINSZ,
            TIOCMGET, TIOCMBIS, TIOCMBIC, TIOCMSET, TIOCDRAIN, TIOCSBRK, TIOCCBRK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_packet_flags_power_of_two() {
        // TIOCPKT_DATA is 0, so exclude it
        let flags: [u8; 6] = [
            TIOCPKT_FLUSHREAD,
            TIOCPKT_FLUSHWRITE,
            TIOCPKT_STOP,
            TIOCPKT_START,
            TIOCPKT_NOSTOP,
            TIOCPKT_DOSTOP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_packet_flags_no_overlap() {
        let flags: [u8; 6] = [
            TIOCPKT_FLUSHREAD,
            TIOCPKT_FLUSHWRITE,
            TIOCPKT_STOP,
            TIOCPKT_START,
            TIOCPKT_NOSTOP,
            TIOCPKT_DOSTOP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_n_tty_zero() {
        assert_eq!(N_TTY, 0);
    }
}
