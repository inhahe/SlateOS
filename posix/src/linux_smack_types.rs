//! `<linux/smack.h>` — SMACK (Simplified Mandatory Access Control Kernel) constants.
//!
//! SMACK is a simpler alternative to SELinux for mandatory access
//! control. It uses short text labels attached to processes and files,
//! with access rules defined as (subject_label, object_label,
//! permissions) tuples. The rule set is typically much smaller than
//! SELinux policies. SMACK is used in Tizen (Samsung's IoT/mobile
//! OS) and embedded Linux systems where SELinux's complexity is
//! not warranted.

// ---------------------------------------------------------------------------
// SMACK access types
// ---------------------------------------------------------------------------

/// Read access.
pub const SMACK_ACCESS_READ: u32 = 1 << 0;
/// Write access.
pub const SMACK_ACCESS_WRITE: u32 = 1 << 1;
/// Execute access.
pub const SMACK_ACCESS_EXEC: u32 = 1 << 2;
/// Append access.
pub const SMACK_ACCESS_APPEND: u32 = 1 << 3;
/// Transmute (inherit label from parent directory).
pub const SMACK_ACCESS_TRANSMUTE: u32 = 1 << 4;
/// Lock access (flock).
pub const SMACK_ACCESS_LOCK: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// SMACK special labels
// ---------------------------------------------------------------------------

/// Floor label (minimum privilege, default for unlabeled files).
pub const SMACK_LABEL_FLOOR: u32 = 0;
/// Hat label (slightly above floor).
pub const SMACK_LABEL_HAT: u32 = 1;
/// Star label (can read anything, cannot write).
pub const SMACK_LABEL_STAR: u32 = 2;
/// Web label (internet-facing processes).
pub const SMACK_LABEL_WEB: u32 = 3;

// ---------------------------------------------------------------------------
// SMACK sysfs interfaces (/sys/fs/smackfs/)
// ---------------------------------------------------------------------------

/// Load rules (write access rules here).
pub const SMACKFS_LOAD: u32 = 0;
/// Load rules v2 (longer labels, up to 255 chars).
pub const SMACKFS_LOAD2: u32 = 1;
/// Change rule (modify existing rule).
pub const SMACKFS_CHANGE_RULE: u32 = 2;
/// Access check (write subject/object/access, read result).
pub const SMACKFS_ACCESS: u32 = 3;
/// Access check v2 (longer labels).
pub const SMACKFS_ACCESS2: u32 = 4;
/// CIPSO configuration (CIPSO label ↔ SMACK label mapping).
pub const SMACKFS_CIPSO: u32 = 5;
/// CIPSO v2 (longer labels).
pub const SMACKFS_CIPSO2: u32 = 6;
/// Direct network (netlabel → SMACK label).
pub const SMACKFS_NETLABEL: u32 = 7;
/// Ambient label (default for unlabeled network packets).
pub const SMACKFS_AMBIENT: u32 = 8;
/// Onlycap (restrict CAP_MAC_ADMIN to this label only).
pub const SMACKFS_ONLYCAP: u32 = 9;
/// Unconfined label (exempt from SMACK checks).
pub const SMACKFS_UNCONFINED: u32 = 10;
/// Revoke subject (remove all rules for a subject).
pub const SMACKFS_REVOKE: u32 = 11;

// ---------------------------------------------------------------------------
// SMACK label length limits
// ---------------------------------------------------------------------------

/// Maximum label length (v1).
pub const SMACK_LABEL_MAX_V1: u32 = 23;
/// Maximum label length (v2).
pub const SMACK_LABEL_MAX_V2: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_flags_no_overlap() {
        let flags = [
            SMACK_ACCESS_READ,
            SMACK_ACCESS_WRITE,
            SMACK_ACCESS_EXEC,
            SMACK_ACCESS_APPEND,
            SMACK_ACCESS_TRANSMUTE,
            SMACK_ACCESS_LOCK,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_special_labels_distinct() {
        let labels = [
            SMACK_LABEL_FLOOR,
            SMACK_LABEL_HAT,
            SMACK_LABEL_STAR,
            SMACK_LABEL_WEB,
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn test_sysfs_interfaces_distinct() {
        let ifaces = [
            SMACKFS_LOAD,
            SMACKFS_LOAD2,
            SMACKFS_CHANGE_RULE,
            SMACKFS_ACCESS,
            SMACKFS_ACCESS2,
            SMACKFS_CIPSO,
            SMACKFS_CIPSO2,
            SMACKFS_NETLABEL,
            SMACKFS_AMBIENT,
            SMACKFS_ONLYCAP,
            SMACKFS_UNCONFINED,
            SMACKFS_REVOKE,
        ];
        for i in 0..ifaces.len() {
            for j in (i + 1)..ifaces.len() {
                assert_ne!(ifaces[i], ifaces[j]);
            }
        }
    }

    #[test]
    fn test_label_length_limits() {
        assert!(SMACK_LABEL_MAX_V1 < SMACK_LABEL_MAX_V2);
        assert!(SMACK_LABEL_MAX_V1 > 0);
    }
}
