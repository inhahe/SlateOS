//! `<linux/netrom.h>` — NET/ROM amateur radio constants.
//!
//! NET/ROM is a network layer protocol used over AX.25
//! amateur radio links.  These constants define NET/ROM
//! socket options, IOCTL commands, and transport parameters.

// ---------------------------------------------------------------------------
// NET/ROM address family
// ---------------------------------------------------------------------------

/// NET/ROM address family.
pub const AF_NETROM: u32 = 6;
/// NET/ROM protocol family.
pub const PF_NETROM: u32 = AF_NETROM;

// ---------------------------------------------------------------------------
// NET/ROM socket options
// ---------------------------------------------------------------------------

/// T1 timer (transport layer ack timeout).
pub const NETROM_T1: u32 = 1;
/// T2 timer (acknowledgment delay).
pub const NETROM_T2: u32 = 2;
/// N2 retry count.
pub const NETROM_N2: u32 = 3;
/// T4 timer (transport busy).
pub const NETROM_T4: u32 = 6;
/// Idle timer.
pub const NETROM_IDLE: u32 = 7;

// ---------------------------------------------------------------------------
// NET/ROM IOCTL commands
// ---------------------------------------------------------------------------

/// Add route.
pub const SIOCNRGETPARMS: u32 = 0x8970;
/// Set route.
pub const SIOCNRSETPARMS: u32 = 0x8971;
/// Decode (AX.25 callsign → NET/ROM).
pub const SIOCNRDECOBS: u32 = 0x8972;
/// Control transport.
pub const SIOCNRRTCTL: u32 = 0x8973;
/// Get packet counts.
pub const SIOCNRCTLCON: u32 = 0x8974;

// ---------------------------------------------------------------------------
// NET/ROM transport parameters
// ---------------------------------------------------------------------------

/// Default T1 timer value (10 seconds, in 100ms units).
pub const NR_DEFAULT_T1: u32 = 120;
/// Default T2 timer value (3 seconds, in 100ms units).
pub const NR_DEFAULT_T2: u32 = 30;
/// Default N2 count.
pub const NR_DEFAULT_N2: u32 = 3;
/// Default T4 value (180 seconds, in 100ms units).
pub const NR_DEFAULT_T4: u32 = 1800;
/// Default idle timeout (20 minutes, in 100ms units).
pub const NR_DEFAULT_IDLE: u32 = 12000;
/// Default window size.
pub const NR_DEFAULT_WINDOW: u32 = 4;
/// Maximum window size.
pub const NR_MAX_WINDOW: u32 = 127;
/// Default packet length.
pub const NR_DEFAULT_PACLEN: u32 = 236;
/// Maximum quality.
pub const NR_MAX_QUALITY: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_netrom() {
        assert_eq!(AF_NETROM, 6);
        assert_eq!(PF_NETROM, AF_NETROM);
    }

    #[test]
    fn test_sockopts_distinct() {
        let opts = [NETROM_T1, NETROM_T2, NETROM_N2, NETROM_T4, NETROM_IDLE];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            SIOCNRGETPARMS,
            SIOCNRSETPARMS,
            SIOCNRDECOBS,
            SIOCNRRTCTL,
            SIOCNRCTLCON,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_defaults() {
        assert!(NR_DEFAULT_WINDOW <= NR_MAX_WINDOW);
        assert!(NR_DEFAULT_T1 > 0);
        assert!(NR_DEFAULT_N2 > 0);
    }

    #[test]
    fn test_max_quality() {
        assert_eq!(NR_MAX_QUALITY, 255);
    }

    #[test]
    fn test_max_window() {
        assert_eq!(NR_MAX_WINDOW, 127);
    }

    #[test]
    fn test_default_paclen() {
        assert_eq!(NR_DEFAULT_PACLEN, 236);
    }
}
