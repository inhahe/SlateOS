//! `<linux/prctl.h>` — prctl() capability-related command constants.
//!
//! The `prctl()` syscall performs various thread/process control
//! operations. These constants define the capability-related
//! prctl commands for managing the bounding set, ambient
//! capabilities, and securebits.

// ---------------------------------------------------------------------------
// prctl() capability commands
// ---------------------------------------------------------------------------

/// Set the capability bounding set.
pub const PR_CAPBSET_READ: u32 = 23;
/// Drop a capability from bounding set.
pub const PR_CAPBSET_DROP: u32 = 24;

/// Get ambient capability state.
pub const PR_CAP_AMBIENT: u32 = 47;
/// Ambient capability sub-commands.
pub const PR_CAP_AMBIENT_IS_SET: u32 = 1;
/// Raise an ambient capability.
pub const PR_CAP_AMBIENT_RAISE: u32 = 2;
/// Lower an ambient capability.
pub const PR_CAP_AMBIENT_LOWER: u32 = 3;
/// Clear all ambient capabilities.
pub const PR_CAP_AMBIENT_CLEAR_ALL: u32 = 4;

// ---------------------------------------------------------------------------
// prctl() securebits commands
// ---------------------------------------------------------------------------

/// Get current securebits.
pub const PR_GET_SECUREBITS: u32 = 27;
/// Set securebits.
pub const PR_SET_SECUREBITS: u32 = 28;

// ---------------------------------------------------------------------------
// prctl() no_new_privs
// ---------------------------------------------------------------------------

/// Get no_new_privs flag.
pub const PR_GET_NO_NEW_PRIVS: u32 = 39;
/// Set no_new_privs flag (irreversible).
pub const PR_SET_NO_NEW_PRIVS: u32 = 38;

// ---------------------------------------------------------------------------
// prctl() keepcaps
// ---------------------------------------------------------------------------

/// Get keep-capabilities flag.
pub const PR_GET_KEEPCAPS: u32 = 7;
/// Set keep-capabilities flag.
pub const PR_SET_KEEPCAPS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capbset_commands() {
        assert_eq!(PR_CAPBSET_READ, 23);
        assert_eq!(PR_CAPBSET_DROP, 24);
        assert_ne!(PR_CAPBSET_READ, PR_CAPBSET_DROP);
    }

    #[test]
    fn test_ambient_subcommands_distinct() {
        let cmds = [
            PR_CAP_AMBIENT_IS_SET, PR_CAP_AMBIENT_RAISE,
            PR_CAP_AMBIENT_LOWER, PR_CAP_AMBIENT_CLEAR_ALL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_securebits_commands() {
        assert_ne!(PR_GET_SECUREBITS, PR_SET_SECUREBITS);
    }

    #[test]
    fn test_no_new_privs() {
        assert_ne!(PR_GET_NO_NEW_PRIVS, PR_SET_NO_NEW_PRIVS);
    }

    #[test]
    fn test_keepcaps() {
        assert_eq!(PR_GET_KEEPCAPS, 7);
        assert_eq!(PR_SET_KEEPCAPS, 8);
    }
}
