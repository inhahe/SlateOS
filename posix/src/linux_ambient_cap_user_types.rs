//! `<sys/prctl.h>` — ambient-capability prctl operations.
//!
//! Linux 4.3 added the "ambient" capability set: capabilities that a
//! non-root process can inherit across execve into non-suid programs.
//! Userspace manipulates this set via `prctl(PR_CAP_AMBIENT, op, …)`.

// ---------------------------------------------------------------------------
// `prctl` first argument
// ---------------------------------------------------------------------------

pub const PR_CAP_AMBIENT: i32 = 47;

// ---------------------------------------------------------------------------
// Sub-operations passed as the second `prctl` argument
// ---------------------------------------------------------------------------

pub const PR_CAP_AMBIENT_IS_SET: u32 = 1;
pub const PR_CAP_AMBIENT_RAISE: u32 = 2;
pub const PR_CAP_AMBIENT_LOWER: u32 = 3;
pub const PR_CAP_AMBIENT_CLEAR_ALL: u32 = 4;

// ---------------------------------------------------------------------------
// /proc/<pid>/status field names
// ---------------------------------------------------------------------------

pub const PROC_STATUS_CAPAMB: &str = "CapAmb:";

// ---------------------------------------------------------------------------
// "no-new-privs" interaction
// ---------------------------------------------------------------------------

pub const PR_SET_NO_NEW_PRIVS: i32 = 38;
pub const PR_GET_NO_NEW_PRIVS: i32 = 39;
pub const PR_NO_NEW_PRIVS_ENABLE: u32 = 1;
pub const PR_NO_NEW_PRIVS_DISABLE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pr_cap_ambient_is_47() {
        // Stable ABI value since kernel 4.3.
        assert_eq!(PR_CAP_AMBIENT, 47);
    }

    #[test]
    fn test_ambient_subops_dense_1_to_4() {
        let ops = [
            PR_CAP_AMBIENT_IS_SET,
            PR_CAP_AMBIENT_RAISE,
            PR_CAP_AMBIENT_LOWER,
            PR_CAP_AMBIENT_CLEAR_ALL,
        ];
        for (i, &v) in ops.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_status_field_format() {
        // /proc/<pid>/status fields end with ':'.
        assert!(PROC_STATUS_CAPAMB.ends_with(':'));
        assert_eq!(PROC_STATUS_CAPAMB, "CapAmb:");
    }

    #[test]
    fn test_nnp_get_set_paired_and_dense() {
        // PR_SET_NO_NEW_PRIVS and its getter are adjacent prctl ops.
        assert_eq!(PR_GET_NO_NEW_PRIVS - PR_SET_NO_NEW_PRIVS, 1);
        assert_eq!(PR_SET_NO_NEW_PRIVS, 38);
    }

    #[test]
    fn test_nnp_enable_disable_boolean() {
        // The flag is a boolean represented as 0 or 1.
        assert_eq!(PR_NO_NEW_PRIVS_ENABLE, 1);
        assert_eq!(PR_NO_NEW_PRIVS_DISABLE, 0);
        assert_ne!(PR_NO_NEW_PRIVS_ENABLE, PR_NO_NEW_PRIVS_DISABLE);
    }
}
