//! `batman` userspace control socket constants.
//!
//! The legacy `batmand` daemon exposed a Unix-domain control socket
//! (`/var/run/batmand.socket`) so utilities like `batmand-vis` and the
//! `batctl` legacy modes could query routing tables. The wire format is
//! a tiny binary protocol.

// ---------------------------------------------------------------------------
// Default control-socket and pid-file paths
// ---------------------------------------------------------------------------

pub const BATMAND_SOCKET_PATH: &str = "/var/run/batmand.socket";
pub const BATMAND_PID_FILE: &str = "/var/run/batmand.pid";
pub const BATMAND_DEFAULT_LOG_FILE: &str = "/var/log/batmand.log";

// ---------------------------------------------------------------------------
// Control-socket command opcodes
// ---------------------------------------------------------------------------

pub const BATMAN_CMD_HELP: u8 = b'h';
pub const BATMAN_CMD_VERSION: u8 = b'v';
pub const BATMAN_CMD_QUIT: u8 = b'q';
pub const BATMAN_CMD_LOGLEVEL: u8 = b'd';
pub const BATMAN_CMD_INTERFACE: u8 = b'i';
pub const BATMAN_CMD_ORIGINATORS: u8 = b'o';
pub const BATMAN_CMD_GATEWAYS: u8 = b'g';
pub const BATMAN_CMD_ROUTING_CLASS: u8 = b'r';
pub const BATMAN_CMD_PREFERRED_GW: u8 = b'p';

// ---------------------------------------------------------------------------
// Log levels (`-d N`)
// ---------------------------------------------------------------------------

pub const BATMAN_LOG_NONE: u8 = 0;
pub const BATMAN_LOG_PROFILE: u8 = 1;
pub const BATMAN_LOG_ROUTES: u8 = 2;
pub const BATMAN_LOG_GATEWAYS: u8 = 3;
pub const BATMAN_LOG_DEBUG: u8 = 4;

// ---------------------------------------------------------------------------
// Routing classes (`-r N`)
// ---------------------------------------------------------------------------

/// No automatic gateway selection.
pub const BATMAN_RC_NONE: u8 = 0;
/// Pick fastest gateway (lowest TQ-based metric).
pub const BATMAN_RC_FAST: u8 = 1;
/// Pick most-stable gateway.
pub const BATMAN_RC_STABLE: u8 = 2;
/// Combine fast + stable heuristics.
pub const BATMAN_RC_FAST_STABLE: u8 = 3;

// ---------------------------------------------------------------------------
// Visualization frame magic for `vis` daemon
// ---------------------------------------------------------------------------

pub const BATMAN_VIS_MAGIC: u8 = 0x42;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_paths() {
        assert!(BATMAND_SOCKET_PATH.starts_with("/var/run/"));
        assert!(BATMAND_PID_FILE.starts_with("/var/run/"));
        assert!(BATMAND_DEFAULT_LOG_FILE.starts_with("/var/log/"));
        assert!(BATMAND_SOCKET_PATH.ends_with(".socket"));
        assert!(BATMAND_PID_FILE.ends_with(".pid"));
        assert!(BATMAND_DEFAULT_LOG_FILE.ends_with(".log"));
    }

    #[test]
    fn test_cmd_letters_are_lowercase_ascii() {
        let c = [
            BATMAN_CMD_HELP,
            BATMAN_CMD_VERSION,
            BATMAN_CMD_QUIT,
            BATMAN_CMD_LOGLEVEL,
            BATMAN_CMD_INTERFACE,
            BATMAN_CMD_ORIGINATORS,
            BATMAN_CMD_GATEWAYS,
            BATMAN_CMD_ROUTING_CLASS,
            BATMAN_CMD_PREFERRED_GW,
        ];
        for &v in &c {
            assert!(v.is_ascii_lowercase());
        }
        // All distinct.
        for (i, &a) in c.iter().enumerate() {
            for &b in &c[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_log_levels_dense_0_to_4() {
        let l = [
            BATMAN_LOG_NONE,
            BATMAN_LOG_PROFILE,
            BATMAN_LOG_ROUTES,
            BATMAN_LOG_GATEWAYS,
            BATMAN_LOG_DEBUG,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // NONE = 0 turns logging off entirely.
        assert_eq!(BATMAN_LOG_NONE, 0);
        // DEBUG is the most verbose.
        assert_eq!(BATMAN_LOG_DEBUG, *l.iter().max().unwrap());
    }

    #[test]
    fn test_routing_classes_dense_0_to_3() {
        let r = [
            BATMAN_RC_NONE,
            BATMAN_RC_FAST,
            BATMAN_RC_STABLE,
            BATMAN_RC_FAST_STABLE,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // FAST_STABLE is the bitwise OR of FAST and STABLE.
        assert_eq!(BATMAN_RC_FAST | BATMAN_RC_STABLE, BATMAN_RC_FAST_STABLE);
    }

    #[test]
    fn test_vis_magic_is_ascii_b() {
        // Magic byte = 'B' (uppercase) — chosen for human-readable
        // packet captures.
        assert_eq!(BATMAN_VIS_MAGIC, b'B');
        assert_eq!(BATMAN_VIS_MAGIC, 0x42);
    }
}
