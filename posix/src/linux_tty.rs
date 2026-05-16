//! `<linux/tty.h>` — TTY line discipline and ioctl constants.
//!
//! Line disciplines process data between the TTY driver and userspace.
//! The standard N_TTY line discipline provides canonical mode editing,
//! echo, and signal generation. Other disciplines handle protocols
//! like SLIP, PPP, and Bluetooth HCI.

// ---------------------------------------------------------------------------
// Line disciplines (N_*)
// ---------------------------------------------------------------------------

/// Standard TTY line discipline (canonical mode).
pub const N_TTY: i32 = 0;
/// SLIP (Serial Line IP).
pub const N_SLIP: i32 = 1;
/// Mouse systems protocol.
pub const N_MOUSE: i32 = 2;
/// PPP (Point-to-Point Protocol).
pub const N_PPP: i32 = 3;
/// Strip (Starmode Radio IP).
pub const N_STRIP: i32 = 4;
/// AX.25 (amateur radio).
pub const N_AX25: i32 = 5;
/// X.25 async (X.25 over serial).
pub const N_X25: i32 = 6;
/// 6pack (amateur radio).
pub const N_6PACK: i32 = 7;
/// HDLC for Mitsumi CD-ROM.
pub const N_MASC: i32 = 8;
/// IrDA.
pub const N_IRDA: i32 = 11;
/// SLIP compressed.
pub const N_SMSBLOCK: i32 = 12;
/// HDLC/sync.
pub const N_HDLC: i32 = 13;
/// Bluetooth HCI.
pub const N_HCI: i32 = 15;
/// GSM MUX.
pub const N_GSM0710: i32 = 21;
/// Number of line disciplines.
pub const NR_LDISCS: i32 = 30;

// ---------------------------------------------------------------------------
// TTY ioctl commands (from asm-generic)
// ---------------------------------------------------------------------------

/// Get window size.
pub const TIOCGWINSZ: u64 = 0x5413;
/// Set window size.
pub const TIOCSWINSZ: u64 = 0x5414;
/// Get exclusive mode.
pub const TIOCGEXCL: u64 = 0x5440;
/// Set exclusive mode.
pub const TIOCEXCL: u64 = 0x540C;
/// Clear exclusive mode.
pub const TIOCNXCL: u64 = 0x540D;
/// Set controlling terminal.
pub const TIOCSCTTY: u64 = 0x540E;
/// Get foreground process group.
pub const TIOCGPGRP: u64 = 0x540F;
/// Set foreground process group.
pub const TIOCSPGRP: u64 = 0x5410;
/// Get number of bytes in output buffer.
pub const TIOCOUTQ: u64 = 0x5411;
/// Simulate terminal input.
pub const TIOCSTI: u64 = 0x5412;
/// Get line discipline.
pub const TIOCGETD: u64 = 0x5424;
/// Set line discipline.
pub const TIOCSETD: u64 = 0x5423;
/// Get session ID.
pub const TIOCGSID: u64 = 0x5429;
/// Get packet mode.
pub const TIOCGPKT: u64 = 0x80045438;
/// Get PTY lock.
pub const TIOCGPTLCK: u64 = 0x80045439;
/// Get PTY peer.
pub const TIOCGPTPEER: u64 = 0x5441;

// ---------------------------------------------------------------------------
// Packet mode bits (TIOCPKT_*)
// ---------------------------------------------------------------------------

/// Data.
pub const TIOCPKT_DATA: u8 = 0;
/// Flush read.
pub const TIOCPKT_FLUSHREAD: u8 = 1;
/// Flush write.
pub const TIOCPKT_FLUSHWRITE: u8 = 2;
/// Stop output.
pub const TIOCPKT_STOP: u8 = 4;
/// Start output.
pub const TIOCPKT_START: u8 = 8;
/// No stop on SIGINT/SIGQUIT.
pub const TIOCPKT_NOSTOP: u8 = 16;
/// Do stop on SIGINT/SIGQUIT.
pub const TIOCPKT_DOSTOP: u8 = 32;
/// IOCTL notification.
pub const TIOCPKT_IOCTL: u8 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_disciplines_distinct() {
        let ldiscs = [
            N_TTY, N_SLIP, N_MOUSE, N_PPP, N_STRIP,
            N_AX25, N_X25, N_6PACK, N_MASC,
            N_IRDA, N_HDLC, N_HCI, N_GSM0710,
        ];
        for i in 0..ldiscs.len() {
            for j in (i + 1)..ldiscs.len() {
                assert_ne!(ldiscs[i], ldiscs[j]);
            }
        }
    }

    #[test]
    fn test_n_tty_zero() {
        assert_eq!(N_TTY, 0);
    }

    #[test]
    fn test_tty_ioctls_distinct() {
        let ioctls = [
            TIOCGWINSZ, TIOCSWINSZ, TIOCEXCL, TIOCNXCL,
            TIOCSCTTY, TIOCGPGRP, TIOCSPGRP, TIOCOUTQ,
            TIOCSTI, TIOCGETD, TIOCSETD, TIOCGSID,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_packet_mode_bits() {
        assert_eq!(TIOCPKT_DATA, 0);
        assert_eq!(TIOCPKT_FLUSHREAD, 1);
        assert_eq!(TIOCPKT_FLUSHWRITE, 2);
        assert_eq!(TIOCPKT_STOP, 4);
        assert_eq!(TIOCPKT_START, 8);
    }

    #[test]
    fn test_nr_ldiscs() {
        assert!(NR_LDISCS >= N_GSM0710);
    }
}
