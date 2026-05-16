//! `<linux/audit.h>` — kernel audit subsystem constants.
//!
//! Provides message types, architecture constants, and field
//! identifiers for the Linux audit framework.

// ---------------------------------------------------------------------------
// Audit message types
// ---------------------------------------------------------------------------

/// Syscall event.
pub const AUDIT_SYSCALL: u32 = 1300;
/// Path name.
pub const AUDIT_PATH: u32 = 1302;
/// IPC record.
pub const AUDIT_IPC: u32 = 1303;
/// Socket address.
pub const AUDIT_SOCKADDR: u32 = 1306;
/// Socket call.
pub const AUDIT_SOCKETCALL: u32 = 1304;
/// Configuration change.
pub const AUDIT_CONFIG_CHANGE: u32 = 1305;
/// CWD record.
pub const AUDIT_CWD: u32 = 1307;
/// execve arguments.
pub const AUDIT_EXECVE: u32 = 1309;
/// Integrity check.
pub const AUDIT_INTEGRITY_DATA: u32 = 1800;
/// User space message.
pub const AUDIT_USER: u32 = 1100;
/// User login.
pub const AUDIT_USER_LOGIN: u32 = 1112;
/// User logout.
pub const AUDIT_USER_LOGOUT: u32 = 1113;
/// User authentication.
pub const AUDIT_USER_AUTH: u32 = 1100;
/// Anomaly event.
pub const AUDIT_ANOM_PROMISCUOUS: u32 = 1700;
/// AVC (access vector cache) message.
pub const AUDIT_AVC: u32 = 1400;
/// Seccomp event.
pub const AUDIT_SECCOMP: u32 = 1326;

// ---------------------------------------------------------------------------
// Audit control messages
// ---------------------------------------------------------------------------

/// Get audit status.
pub const AUDIT_GET: u32 = 1000;
/// Set audit status.
pub const AUDIT_SET: u32 = 1001;
/// List rules.
pub const AUDIT_LIST_RULES: u32 = 1013;
/// Add rule.
pub const AUDIT_ADD_RULE: u32 = 1011;
/// Delete rule.
pub const AUDIT_DEL_RULE: u32 = 1012;
/// User message (from user space).
pub const AUDIT_USER_MSG: u32 = 1100;
/// Signal info.
pub const AUDIT_SIGNAL_INFO: u32 = 1010;

// ---------------------------------------------------------------------------
// Architecture constants (AUDIT_ARCH_*)
// ---------------------------------------------------------------------------

/// x86_64 (64-bit).
pub const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
/// i386 (32-bit).
pub const AUDIT_ARCH_I386: u32 = 0x4000_0003;
/// ARM 32-bit.
pub const AUDIT_ARCH_ARM: u32 = 0x4000_0028;
/// AArch64 (ARM 64-bit).
pub const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;
/// RISC-V 64-bit.
pub const AUDIT_ARCH_RISCV64: u32 = 0xC000_00F3;

// ---------------------------------------------------------------------------
// Audit field identifiers
// ---------------------------------------------------------------------------

/// Process ID.
pub const AUDIT_PID: u32 = 0;
/// User ID.
pub const AUDIT_UID: u32 = 1;
/// Effective UID.
pub const AUDIT_EUID: u32 = 2;
/// Saved-set UID.
pub const AUDIT_SUID: u32 = 3;
/// Filesystem UID.
pub const AUDIT_FSUID: u32 = 4;
/// Group ID.
pub const AUDIT_GID: u32 = 5;
/// Effective GID.
pub const AUDIT_EGID: u32 = 6;
/// Login UID.
pub const AUDIT_LOGINUID: u32 = 9;
/// Architecture.
pub const AUDIT_ARCH: u32 = 11;
/// Message type.
pub const AUDIT_MSGTYPE: u32 = 12;
/// Personality.
pub const AUDIT_PERS: u32 = 10;
/// Exit value.
pub const AUDIT_EXIT: u32 = 103;
/// Success flag.
pub const AUDIT_SUCCESS: u32 = 104;

// ---------------------------------------------------------------------------
// Audit filter types
// ---------------------------------------------------------------------------

/// Entry filter (deprecated).
pub const AUDIT_FILTER_ENTRY: u32 = 0x02;
/// Exit filter.
pub const AUDIT_FILTER_EXIT: u32 = 0x04;
/// Task filter.
pub const AUDIT_FILTER_TASK: u32 = 0x01;
/// User filter.
pub const AUDIT_FILTER_USER: u32 = 0x00;
/// Exclude filter.
pub const AUDIT_FILTER_EXCLUDE: u32 = 0x05;
/// Filesystem filter.
pub const AUDIT_FILTER_FS: u32 = 0x06;

// ---------------------------------------------------------------------------
// Audit actions
// ---------------------------------------------------------------------------

/// Never audit.
pub const AUDIT_NEVER: u32 = 0;
/// Possible audit.
pub const AUDIT_POSSIBLE: u32 = 1;
/// Always audit.
pub const AUDIT_ALWAYS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types() {
        assert_eq!(AUDIT_SYSCALL, 1300);
        assert_eq!(AUDIT_PATH, 1302);
        assert_eq!(AUDIT_SECCOMP, 1326);
    }

    #[test]
    fn test_control_messages() {
        assert_eq!(AUDIT_GET, 1000);
        assert_eq!(AUDIT_SET, 1001);
        assert_ne!(AUDIT_ADD_RULE, AUDIT_DEL_RULE);
    }

    #[test]
    fn test_arch_constants() {
        assert_ne!(AUDIT_ARCH_X86_64, AUDIT_ARCH_I386);
        assert_ne!(AUDIT_ARCH_ARM, AUDIT_ARCH_AARCH64);
        // 64-bit archs have bit 31 set (convention EM_* | __AUDIT_ARCH_64BIT).
        assert_ne!(AUDIT_ARCH_X86_64 & 0x8000_0000, 0);
        assert_ne!(AUDIT_ARCH_AARCH64 & 0x8000_0000, 0);
    }

    #[test]
    fn test_field_ids_distinct() {
        let fields = [
            AUDIT_PID, AUDIT_UID, AUDIT_EUID, AUDIT_SUID,
            AUDIT_FSUID, AUDIT_GID, AUDIT_EGID, AUDIT_LOGINUID,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_filter_types_distinct() {
        let filters = [
            AUDIT_FILTER_USER, AUDIT_FILTER_TASK, AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_EXIT, AUDIT_FILTER_EXCLUDE, AUDIT_FILTER_FS,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }

    #[test]
    fn test_audit_actions() {
        assert_eq!(AUDIT_NEVER, 0);
        assert_eq!(AUDIT_POSSIBLE, 1);
        assert_eq!(AUDIT_ALWAYS, 2);
    }
}
