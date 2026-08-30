//! `<linux/fou.h>` — Additional FOU (Foo-over-UDP) constants.
//!
//! Supplementary FOU constants covering encapsulation types,
//! genetlink commands, and UDP port configuration.

// ---------------------------------------------------------------------------
// FOU encapsulation types
// ---------------------------------------------------------------------------

/// Direct encapsulation (FOU).
pub const FOU_ENCAP_DIRECT: u32 = 0;
/// GUE (Generic UDP Encapsulation).
pub const FOU_ENCAP_GUE: u32 = 1;

// ---------------------------------------------------------------------------
// FOU genetlink commands
// ---------------------------------------------------------------------------

/// Unspec command.
pub const FOU_CMD_UNSPEC: u32 = 0;
/// Add FOU port.
pub const FOU_CMD_ADD: u32 = 1;
/// Delete FOU port.
pub const FOU_CMD_DEL: u32 = 2;
/// Get FOU port info.
pub const FOU_CMD_GET: u32 = 3;

// ---------------------------------------------------------------------------
// GUE header flags
// ---------------------------------------------------------------------------

/// GUE version mask.
pub const GUE_VERSION_MASK: u32 = 0x03;
/// GUE has control flag.
pub const GUE_FLAG_CONTROL: u32 = 1 << 5;
/// GUE has payload proto next header.
pub const GUE_FLAG_PRIV: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// GUE header sizes
// ---------------------------------------------------------------------------

/// GUE base header length (bytes).
pub const GUE_BASE_HLEN: u32 = 4;
/// Maximum GUE option length (bytes).
pub const GUE_MAX_OPT_LEN: u32 = 124;

// ---------------------------------------------------------------------------
// FOU UDP ports
// ---------------------------------------------------------------------------

/// Default FOU port (no standard default, commonly 5555).
pub const FOU_DEFAULT_PORT: u16 = 5555;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(FOU_ENCAP_DIRECT, FOU_ENCAP_GUE);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [FOU_CMD_UNSPEC, FOU_CMD_ADD, FOU_CMD_DEL, FOU_CMD_GET];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_gue_constants() {
        assert_eq!(GUE_BASE_HLEN, 4);
        assert!(GUE_MAX_OPT_LEN > 0);
    }

    #[test]
    fn test_default_port() {
        assert!(FOU_DEFAULT_PORT > 0);
    }
}
