//! `<termios.h>` — terminal line-discipline control.
//!
//! `termios` is the POSIX terminal driver knob set: input mode
//! (`c_iflag`), output mode (`c_oflag`), control mode (`c_cflag`),
//! local mode (`c_lflag`), control characters, and baud rate.
//! Every shell, ssh, vim, and serial-port utility pokes these.

// ---------------------------------------------------------------------------
// `c_iflag` — input modes
// ---------------------------------------------------------------------------

pub const IGNBRK: u32 = 0o000001;
pub const BRKINT: u32 = 0o000002;
pub const IGNPAR: u32 = 0o000004;
pub const PARMRK: u32 = 0o000010;
pub const INPCK: u32 = 0o000020;
pub const ISTRIP: u32 = 0o000040;
pub const INLCR: u32 = 0o000100;
pub const IGNCR: u32 = 0o000200;
pub const ICRNL: u32 = 0o000400;
pub const IUCLC: u32 = 0o001000;
pub const IXON: u32 = 0o002000;
pub const IXANY: u32 = 0o004000;
pub const IXOFF: u32 = 0o010000;
pub const IMAXBEL: u32 = 0o020000;
pub const IUTF8: u32 = 0o040000;

// ---------------------------------------------------------------------------
// `c_oflag` — output modes
// ---------------------------------------------------------------------------

pub const OPOST: u32 = 0o000001;
pub const OLCUC: u32 = 0o000002;
pub const ONLCR: u32 = 0o000004;
pub const OCRNL: u32 = 0o000010;
pub const ONOCR: u32 = 0o000020;
pub const ONLRET: u32 = 0o000040;
pub const OFILL: u32 = 0o000100;
pub const OFDEL: u32 = 0o000200;

// ---------------------------------------------------------------------------
// `c_cflag` — control modes
// ---------------------------------------------------------------------------

pub const CSIZE: u32 = 0o000060;
pub const CS5: u32 = 0o000000;
pub const CS6: u32 = 0o000020;
pub const CS7: u32 = 0o000040;
pub const CS8: u32 = 0o000060;
pub const CSTOPB: u32 = 0o000100;
pub const CREAD: u32 = 0o000200;
pub const PARENB: u32 = 0o000400;
pub const PARODD: u32 = 0o001000;
pub const HUPCL: u32 = 0o002000;
pub const CLOCAL: u32 = 0o004000;
pub const CRTSCTS: u32 = 0o020000000000;

// ---------------------------------------------------------------------------
// `c_lflag` — local modes
// ---------------------------------------------------------------------------

pub const ISIG: u32 = 0o000001;
pub const ICANON: u32 = 0o000002;
pub const ECHO: u32 = 0o000010;
pub const ECHOE: u32 = 0o000020;
pub const ECHOK: u32 = 0o000040;
pub const ECHONL: u32 = 0o000100;
pub const NOFLSH: u32 = 0o000200;
pub const TOSTOP: u32 = 0o000400;
pub const IEXTEN: u32 = 0o100000;

// ---------------------------------------------------------------------------
// `c_cc[]` indices — special characters
// ---------------------------------------------------------------------------

pub const VINTR: usize = 0;
pub const VQUIT: usize = 1;
pub const VERASE: usize = 2;
pub const VKILL: usize = 3;
pub const VEOF: usize = 4;
pub const VTIME: usize = 5;
pub const VMIN: usize = 6;
pub const VSWTC: usize = 7;
pub const VSTART: usize = 8;
pub const VSTOP: usize = 9;
pub const VSUSP: usize = 10;
pub const VEOL: usize = 11;
pub const VREPRINT: usize = 12;
pub const VDISCARD: usize = 13;
pub const VWERASE: usize = 14;
pub const VLNEXT: usize = 15;
pub const VEOL2: usize = 16;

/// `NCCS` — length of `c_cc[]` on Linux.
pub const NCCS: usize = 19;

// ---------------------------------------------------------------------------
// `tcsetattr` "when" argument
// ---------------------------------------------------------------------------

pub const TCSANOW: u32 = 0;
pub const TCSADRAIN: u32 = 1;
pub const TCSAFLUSH: u32 = 2;

// ---------------------------------------------------------------------------
// Baud rates (Bnnnn — encoded in `c_cflag` low 4 bits via `CBAUD`)
// ---------------------------------------------------------------------------

pub const B0: u32 = 0o000000;
pub const B50: u32 = 0o000001;
pub const B75: u32 = 0o000002;
pub const B110: u32 = 0o000003;
pub const B134: u32 = 0o000004;
pub const B150: u32 = 0o000005;
pub const B200: u32 = 0o000006;
pub const B300: u32 = 0o000007;
pub const B600: u32 = 0o000010;
pub const B1200: u32 = 0o000011;
pub const B1800: u32 = 0o000012;
pub const B2400: u32 = 0o000013;
pub const B4800: u32 = 0o000014;
pub const B9600: u32 = 0o000015;
pub const B19200: u32 = 0o000016;
pub const B38400: u32 = 0o000017;
pub const B57600: u32 = 0o010001;
pub const B115200: u32 = 0o010002;
pub const B230400: u32 = 0o010003;
pub const B460800: u32 = 0o010004;
pub const B921600: u32 = 0o010007;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csize_covers_cs5_to_cs8() {
        // CSIZE is the 2-bit mask covering CS5..CS8.
        assert_eq!(CSIZE, CS5 | CS6 | CS7 | CS8);
        assert_eq!(CS8, CSIZE);
        assert_eq!(CS5, 0);
    }

    #[test]
    fn test_iflag_bits_distinct() {
        let f = [
            IGNBRK, BRKINT, IGNPAR, PARMRK, INPCK, ISTRIP, INLCR, IGNCR, ICRNL, IUCLC, IXON, IXANY,
            IXOFF, IMAXBEL, IUTF8,
        ];
        // Every input flag is a single bit, all distinct.
        for v in f {
            assert!(v.is_power_of_two(), "{v:o} should be one bit");
        }
        let mut or = 0u32;
        for v in f {
            or |= v;
        }
        // Low-15-bit dense block 0o000001..0o040000.
        assert_eq!(or, 0o077777);
    }

    #[test]
    fn test_cc_indices_dense_and_fit_in_nccs() {
        let i = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME, VMIN, VSWTC, VSTART, VSTOP, VSUSP, VEOL,
            VREPRINT, VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for (idx, &v) in i.iter().enumerate() {
            assert_eq!(v, idx);
        }
        // c_cc has room for them all (Linux pads to 19).
        assert!(*i.last().expect("non-empty") < NCCS);
        assert_eq!(NCCS, 19);
    }

    #[test]
    fn test_tcsetattr_when_dense_0_to_2() {
        assert_eq!(TCSANOW, 0);
        assert_eq!(TCSADRAIN, 1);
        assert_eq!(TCSAFLUSH, 2);
    }

    #[test]
    fn test_baud_low_15_dense_b0_to_b38400() {
        let b = [
            B0, B50, B75, B110, B134, B150, B200, B300, B600, B1200, B1800, B2400, B4800, B9600,
            B19200, B38400,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Above B38400 the encoding switches to the CBAUDEX namespace
        // (bit 0o010000 set) for B57600 and above.
        assert_eq!(B57600 & 0o010000, 0o010000);
        assert_eq!(B115200 & 0o010000, 0o010000);
        assert_eq!(B921600 & 0o010000, 0o010000);
    }

    #[test]
    fn test_crtscts_in_high_bits() {
        // CRTSCTS lives way up at 0o020000000000 (bit 31), since the
        // low 16 bits are exhausted by other c_cflag bits + baud.
        assert_eq!(CRTSCTS, 1 << 31);
    }
}
