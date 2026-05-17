//! `<linux/apparmor.h>` — AppArmor MAC (Mandatory Access Control) constants.
//!
//! AppArmor confines programs based on per-program profiles that
//! specify which files, network access, capabilities, and other
//! resources a program may use. Profiles are pathname-based (unlike
//! SELinux's label-based model), making them easier to write and
//! audit. AppArmor supports profile stacking, namespace delegation,
//! and a learning/complain mode for developing profiles.

// ---------------------------------------------------------------------------
// AppArmor profile modes
// ---------------------------------------------------------------------------

/// Enforce mode (deny and log violations).
pub const AA_MODE_ENFORCE: u32 = 0;
/// Complain mode (log but don't deny, for profile development).
pub const AA_MODE_COMPLAIN: u32 = 1;
/// Kill mode (kill process on violation).
pub const AA_MODE_KILL: u32 = 2;
/// Unconfined mode (no restrictions).
pub const AA_MODE_UNCONFINED: u32 = 3;

// ---------------------------------------------------------------------------
// AppArmor file permissions
// ---------------------------------------------------------------------------

/// Execute permission.
pub const AA_PERM_EXEC: u32 = 1 << 0;
/// Write permission.
pub const AA_PERM_WRITE: u32 = 1 << 1;
/// Read permission.
pub const AA_PERM_READ: u32 = 1 << 2;
/// Append permission.
pub const AA_PERM_APPEND: u32 = 1 << 3;
/// Create permission (new files).
pub const AA_PERM_CREATE: u32 = 1 << 4;
/// Delete permission (unlink/rmdir).
pub const AA_PERM_DELETE: u32 = 1 << 5;
/// Rename permission.
pub const AA_PERM_RENAME: u32 = 1 << 6;
/// Set attributes permission (chmod/chown).
pub const AA_PERM_SETATTR: u32 = 1 << 7;
/// Get attributes permission (stat).
pub const AA_PERM_GETATTR: u32 = 1 << 8;
/// Link permission (hard link).
pub const AA_PERM_LINK: u32 = 1 << 9;
/// Lock permission (flock).
pub const AA_PERM_LOCK: u32 = 1 << 10;
/// mmap with PROT_EXEC.
pub const AA_PERM_MMAP_EXEC: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// AppArmor exec transition types
// ---------------------------------------------------------------------------

/// Inherit parent's profile.
pub const AA_EXEC_INHERIT: u32 = 0;
/// Transition to named profile.
pub const AA_EXEC_PROFILE: u32 = 1;
/// Unconfined execution.
pub const AA_EXEC_UNCONFINED: u32 = 2;
/// Transition to child profile.
pub const AA_EXEC_CHILD: u32 = 3;

// ---------------------------------------------------------------------------
// AppArmor network access types
// ---------------------------------------------------------------------------

/// Network create (socket).
pub const AA_NET_CREATE: u32 = 1 << 0;
/// Network bind.
pub const AA_NET_BIND: u32 = 1 << 1;
/// Network connect.
pub const AA_NET_CONNECT: u32 = 1 << 2;
/// Network listen.
pub const AA_NET_LISTEN: u32 = 1 << 3;
/// Network accept.
pub const AA_NET_ACCEPT: u32 = 1 << 4;
/// Network send.
pub const AA_NET_SEND: u32 = 1 << 5;
/// Network receive.
pub const AA_NET_RECEIVE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// AppArmor interface (/sys/kernel/security/apparmor/)
// ---------------------------------------------------------------------------

/// Profiles are loaded via this interface.
pub const AA_IFACE_PROFILES: u32 = 0;
/// Features directory (advertises supported features).
pub const AA_IFACE_FEATURES: u32 = 1;
/// Policy namespace root.
pub const AA_IFACE_NS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            AA_MODE_ENFORCE, AA_MODE_COMPLAIN,
            AA_MODE_KILL, AA_MODE_UNCONFINED,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_file_perms_no_overlap() {
        let perms = [
            AA_PERM_EXEC, AA_PERM_WRITE, AA_PERM_READ,
            AA_PERM_APPEND, AA_PERM_CREATE, AA_PERM_DELETE,
            AA_PERM_RENAME, AA_PERM_SETATTR, AA_PERM_GETATTR,
            AA_PERM_LINK, AA_PERM_LOCK, AA_PERM_MMAP_EXEC,
        ];
        for i in 0..perms.len() {
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_exec_transitions_distinct() {
        let trans = [
            AA_EXEC_INHERIT, AA_EXEC_PROFILE,
            AA_EXEC_UNCONFINED, AA_EXEC_CHILD,
        ];
        for i in 0..trans.len() {
            for j in (i + 1)..trans.len() {
                assert_ne!(trans[i], trans[j]);
            }
        }
    }

    #[test]
    fn test_net_perms_no_overlap() {
        let perms = [
            AA_NET_CREATE, AA_NET_BIND, AA_NET_CONNECT,
            AA_NET_LISTEN, AA_NET_ACCEPT, AA_NET_SEND,
            AA_NET_RECEIVE,
        ];
        for i in 0..perms.len() {
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }
}
