//! `<linux/apparmor.h>` — AppArmor LSM securityfs paths and label syntax.
//!
//! AppArmor confines processes by attaching a textual "label" (also
//! called a profile name) that the kernel evaluates against a binary
//! policy. Userspace tooling parses the strings in `/proc/self/attr/*`
//! and submits policy via the AppArmor securityfs.

// ---------------------------------------------------------------------------
// LSM name and procfs label files
// ---------------------------------------------------------------------------

pub const APPARMOR_LSM_NAME: &str = "apparmor";

pub const PROC_ATTR_CURRENT: &str = "/proc/self/attr/current";
pub const PROC_ATTR_EXEC: &str = "/proc/self/attr/exec";
pub const PROC_ATTR_PREV: &str = "/proc/self/attr/prev";
pub const PROC_ATTR_FSCREATE: &str = "/proc/self/attr/fscreate";
pub const PROC_ATTR_KEYCREATE: &str = "/proc/self/attr/keycreate";
pub const PROC_ATTR_SOCKCREATE: &str = "/proc/self/attr/sockcreate";

// ---------------------------------------------------------------------------
// AppArmor securityfs root
// ---------------------------------------------------------------------------

pub const APPARMOR_SECURITYFS: &str = "/sys/kernel/security/apparmor";

pub const APPARMOR_PROFILES: &str = "/sys/kernel/security/apparmor/profiles";
pub const APPARMOR_POLICY_LOAD: &str = "/sys/kernel/security/apparmor/.load";
pub const APPARMOR_POLICY_REPLACE: &str = "/sys/kernel/security/apparmor/.replace";
pub const APPARMOR_POLICY_REMOVE: &str = "/sys/kernel/security/apparmor/.remove";

// ---------------------------------------------------------------------------
// Profile-mode tokens (read from /proc/self/attr/current)
// ---------------------------------------------------------------------------

pub const APPARMOR_MODE_ENFORCE: &str = "enforce";
pub const APPARMOR_MODE_COMPLAIN: &str = "complain";
pub const APPARMOR_MODE_UNCONFINED: &str = "unconfined";
pub const APPARMOR_MODE_DISABLED: &str = "disabled";

// ---------------------------------------------------------------------------
// Profile-name special tokens
// ---------------------------------------------------------------------------

/// Profile namespace delimiter in `:ns:profile` syntax.
pub const APPARMOR_NS_SEPARATOR: u8 = b':';
/// Stacked-label separator (e.g. `parent//child`).
pub const APPARMOR_STACK_SEPARATOR: &str = "//";
/// `null-` prefix denotes the implicit deny-everything fallback.
pub const APPARMOR_NULL_PREFIX: &str = "null-";

// ---------------------------------------------------------------------------
// Maximum profile-name and label lengths (kernel implementation cap)
// ---------------------------------------------------------------------------

pub const APPARMOR_MAX_NAME_LEN: usize = 1_024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsm_name() {
        assert_eq!(APPARMOR_LSM_NAME, "apparmor");
    }

    #[test]
    fn test_proc_attr_paths_under_self_attr() {
        for p in [
            PROC_ATTR_CURRENT,
            PROC_ATTR_EXEC,
            PROC_ATTR_PREV,
            PROC_ATTR_FSCREATE,
            PROC_ATTR_KEYCREATE,
            PROC_ATTR_SOCKCREATE,
        ] {
            assert!(p.starts_with("/proc/self/attr/"));
        }
    }

    #[test]
    fn test_securityfs_paths_consistent() {
        assert!(APPARMOR_PROFILES.starts_with(APPARMOR_SECURITYFS));
        assert!(APPARMOR_POLICY_LOAD.starts_with(APPARMOR_SECURITYFS));
        assert!(APPARMOR_POLICY_REPLACE.starts_with(APPARMOR_SECURITYFS));
        assert!(APPARMOR_POLICY_REMOVE.starts_with(APPARMOR_SECURITYFS));
        // Dotfile policy entry-points (load/replace/remove) are distinct.
        for a in [APPARMOR_POLICY_LOAD, APPARMOR_POLICY_REPLACE, APPARMOR_POLICY_REMOVE] {
            assert!(a.contains("/."));
        }
    }

    #[test]
    fn test_mode_tokens_distinct() {
        let m = [
            APPARMOR_MODE_ENFORCE,
            APPARMOR_MODE_COMPLAIN,
            APPARMOR_MODE_UNCONFINED,
            APPARMOR_MODE_DISABLED,
        ];
        for (i, &a) in m.iter().enumerate() {
            for &b in &m[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_separators_and_max_len() {
        assert_eq!(APPARMOR_NS_SEPARATOR, b':');
        assert_eq!(APPARMOR_STACK_SEPARATOR, "//");
        assert!(APPARMOR_NULL_PREFIX.starts_with("null"));
        // 1 KiB is the standard kernel label cap.
        assert_eq!(APPARMOR_MAX_NAME_LEN, 1024);
        assert!(APPARMOR_MAX_NAME_LEN.is_power_of_two());
    }
}
