//! `<linux/ax25.h>` continuation — AX.25 setsockopt / route ioctl codes.
//!
//! AX.25 packet-radio sockets expose tunables (T1, T2, T3, idle, N2,
//! paclen, backoff…) via `setsockopt(SOL_AX25, …)`. Userspace tools
//! (`ax25-tools`, `kissattach`) write these directly.

// ---------------------------------------------------------------------------
// AX.25 socket options (`SOL_AX25`)
// ---------------------------------------------------------------------------

pub const AX25_WINDOW: u32 = 1;
pub const AX25_T1: u32 = 2;
pub const AX25_N2: u32 = 3;
pub const AX25_T3: u32 = 4;
pub const AX25_T2: u32 = 5;
pub const AX25_BACKOFF: u32 = 6;
pub const AX25_EXTSEQ: u32 = 7;
pub const AX25_PIDINCL: u32 = 8;
pub const AX25_IDLE: u32 = 9;
pub const AX25_PACLEN: u32 = 10;
pub const AX25_IAMDIGI: u32 = 12;
pub const AX25_KILL: u32 = 99;

// ---------------------------------------------------------------------------
// AX.25 route ioctl operations
// ---------------------------------------------------------------------------

pub const SIOCAX25GETUID: u32 = 0x89E0;
pub const SIOCAX25ADDUID: u32 = 0x89E1;
pub const SIOCAX25DELUID: u32 = 0x89E2;
pub const SIOCAX25NOUID: u32 = 0x89E3;
pub const SIOCAX25OPTRT: u32 = 0x89E7;
pub const SIOCAX25CTLCON: u32 = 0x89E8;
pub const SIOCAX25GETINFOOLD: u32 = 0x89E9;
pub const SIOCAX25ADDFWD: u32 = 0x89EA;
pub const SIOCAX25DELFWD: u32 = 0x89EB;
pub const SIOCAX25DEVCTL: u32 = 0x89EC;
pub const SIOCAX25GETINFO: u32 = 0x89ED;

// ---------------------------------------------------------------------------
// AX25_NOUID policies
// ---------------------------------------------------------------------------

pub const AX25_NOUID_DEFAULT: u32 = 0;
pub const AX25_NOUID_BLOCK: u32 = 1;

// ---------------------------------------------------------------------------
// Default protocol parameters (seconds unless noted)
// ---------------------------------------------------------------------------

pub const AX25_DEF_T1: u32 = 10;
pub const AX25_DEF_T2: u32 = 3;
pub const AX25_DEF_T3: u32 = 300;
pub const AX25_DEF_N2: u32 = 10;
pub const AX25_DEF_IDLE: u32 = 0;
pub const AX25_DEF_PACLEN: u32 = 256;
pub const AX25_DEF_WINDOW: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockopts_distinct() {
        let s = [
            AX25_WINDOW,
            AX25_T1,
            AX25_N2,
            AX25_T3,
            AX25_T2,
            AX25_BACKOFF,
            AX25_EXTSEQ,
            AX25_PIDINCL,
            AX25_IDLE,
            AX25_PACLEN,
            AX25_IAMDIGI,
            AX25_KILL,
        ];
        for (i, &a) in s.iter().enumerate() {
            for &b in &s[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // 1..10 + 12 are in the regular block; KILL=99 is a sentinel.
        assert_eq!(AX25_KILL, 99);
        // 11 was historically dropped — IAMDIGI jumps to 12.
        assert_eq!(AX25_IAMDIGI, 12);
    }

    #[test]
    fn test_ioctl_codes_in_siocdevprivate_range() {
        let i = [
            SIOCAX25GETUID,
            SIOCAX25ADDUID,
            SIOCAX25DELUID,
            SIOCAX25NOUID,
            SIOCAX25OPTRT,
            SIOCAX25CTLCON,
            SIOCAX25GETINFOOLD,
            SIOCAX25ADDFWD,
            SIOCAX25DELFWD,
            SIOCAX25DEVCTL,
            SIOCAX25GETINFO,
        ];
        // All sit in the 0x89E0..0x89EF block.
        for &v in &i {
            assert!((0x89E0..=0x89EF).contains(&v));
        }
        // UID quartet is dense 0x89E0..0x89E3.
        assert_eq!(SIOCAX25ADDUID - SIOCAX25GETUID, 1);
        assert_eq!(SIOCAX25DELUID - SIOCAX25ADDUID, 1);
        assert_eq!(SIOCAX25NOUID - SIOCAX25DELUID, 1);
    }

    #[test]
    fn test_default_timers_relate() {
        // Linux defaults from net/ax25/ax25_subr.c.
        assert_eq!(AX25_DEF_T1, 10);
        assert_eq!(AX25_DEF_T2, 3);
        assert_eq!(AX25_DEF_T3, 300);
        // T3 (link idle) > T1 (frame retx) > T2 (ack delay).
        assert!(AX25_DEF_T3 > AX25_DEF_T1);
        assert!(AX25_DEF_T1 > AX25_DEF_T2);
    }

    #[test]
    fn test_default_paclen_window() {
        // Window=2 means up to two unack'ed I-frames.
        assert_eq!(AX25_DEF_WINDOW, 2);
        // Default PACLEN of 256 bytes matches the standard packet-radio
        // recommendation; ham operators sometimes raise it to 512.
        assert_eq!(AX25_DEF_PACLEN, 256);
        assert!(AX25_DEF_PACLEN.is_power_of_two());
        // N2 of 10 attempts before declaring a link dead.
        assert_eq!(AX25_DEF_N2, 10);
        // IDLE=0 disables the idle-disconnect timer by default.
        assert_eq!(AX25_DEF_IDLE, 0);
    }

    #[test]
    fn test_nouid_policy_pair() {
        assert_eq!(AX25_NOUID_DEFAULT, 0);
        assert_eq!(AX25_NOUID_BLOCK, 1);
        assert_eq!(AX25_NOUID_BLOCK - AX25_NOUID_DEFAULT, 1);
    }
}
