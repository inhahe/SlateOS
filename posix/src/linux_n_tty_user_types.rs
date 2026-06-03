//! `<linux/n_tty.h>` — the canonical line discipline (`N_TTY`).
//!
//! Every PTY, serial console, and virtual terminal goes through a
//! "line discipline" — the kernel layer that turns a raw byte stream
//! into cooked input (echo, line editing, signal generation). `N_TTY`
//! is the default; SLIP, PPP, and HCI use different disciplines. The
//! constants here come from `<linux/tty.h>` and POSIX `termios`.

// ---------------------------------------------------------------------------
// Line-discipline identifiers (passed to `ioctl(TIOCSETD)`)
// ---------------------------------------------------------------------------

pub const N_TTY: u32 = 0;
pub const N_SLIP: u32 = 1;
pub const N_MOUSE: u32 = 2;
pub const N_PPP: u32 = 3;
pub const N_STRIP: u32 = 4;
pub const N_AX25: u32 = 5;
pub const N_X25: u32 = 6;
pub const N_6PACK: u32 = 7;
pub const N_MASC: u32 = 8;
pub const N_R3964: u32 = 9;
pub const N_PROFIBUS_FDL: u32 = 10;
pub const N_IRDA: u32 = 11;
pub const N_SMSBLOCK: u32 = 12;
pub const N_HDLC: u32 = 13;
pub const N_SYNC_PPP: u32 = 14;
pub const N_HCI: u32 = 15;
pub const N_GIGASET_M101: u32 = 16;
pub const N_SLCAN: u32 = 17;
pub const N_PPS: u32 = 18;
pub const N_V253: u32 = 19;
pub const N_CAIF: u32 = 20;
pub const N_GSM0710: u32 = 21;
pub const N_TI_WL: u32 = 22;
pub const N_TRACESINK: u32 = 23;
pub const N_TRACEROUTER: u32 = 24;
pub const N_NCI: u32 = 25;
pub const N_SPEAKUP: u32 = 26;
pub const N_NULL: u32 = 27;
pub const N_MCTP: u32 = 28;
pub const N_DEVELOPMENT: u32 = 29;
pub const N_CAN327: u32 = 30;

/// Total number of line disciplines (upper bound).
pub const NR_LDISCS: u32 = 31;

// ---------------------------------------------------------------------------
// `c_cc` field indices used by `N_TTY` (POSIX V* indices)
// ---------------------------------------------------------------------------

pub const VINTR: u32 = 0;
pub const VQUIT: u32 = 1;
pub const VERASE: u32 = 2;
pub const VKILL: u32 = 3;
pub const VEOF: u32 = 4;
pub const VTIME: u32 = 5;
pub const VMIN: u32 = 6;
pub const VSWTC: u32 = 7;
pub const VSTART: u32 = 8;
pub const VSTOP: u32 = 9;
pub const VSUSP: u32 = 10;
pub const VEOL: u32 = 11;
pub const VREPRINT: u32 = 12;
pub const VDISCARD: u32 = 13;
pub const VWERASE: u32 = 14;
pub const VLNEXT: u32 = 15;
pub const VEOL2: u32 = 16;
pub const NCCS: u32 = 19;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_n_tty_is_zero_and_within_range() {
        // The default discipline (cooked terminal) is always id 0.
        assert_eq!(N_TTY, 0);
        assert!(N_DEVELOPMENT < NR_LDISCS);
    }

    #[test]
    fn test_ldiscs_dense_0_to_30() {
        let d = [
            N_TTY,
            N_SLIP,
            N_MOUSE,
            N_PPP,
            N_STRIP,
            N_AX25,
            N_X25,
            N_6PACK,
            N_MASC,
            N_R3964,
            N_PROFIBUS_FDL,
            N_IRDA,
            N_SMSBLOCK,
            N_HDLC,
            N_SYNC_PPP,
            N_HCI,
            N_GIGASET_M101,
            N_SLCAN,
            N_PPS,
            N_V253,
            N_CAIF,
            N_GSM0710,
            N_TI_WL,
            N_TRACESINK,
            N_TRACEROUTER,
            N_NCI,
            N_SPEAKUP,
            N_NULL,
            N_MCTP,
            N_DEVELOPMENT,
            N_CAN327,
        ];
        for (i, &v) in d.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(NR_LDISCS, 31);
    }

    #[test]
    fn test_cc_indices_dense_0_to_16() {
        let v = [
            VINTR, VQUIT, VERASE, VKILL, VEOF, VTIME, VMIN, VSWTC, VSTART, VSTOP, VSUSP, VEOL,
            VREPRINT, VDISCARD, VWERASE, VLNEXT, VEOL2,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_nccs_holds_all_indices() {
        // NCCS must be larger than every used index.
        assert!(NCCS > VEOL2);
        assert_eq!(NCCS, 19);
    }
}
