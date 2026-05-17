//! `<linux/audit.h>` — Linux audit subsystem constants.
//!
//! The audit framework records security-relevant events: syscalls,
//! file access, authentication, privilege changes, and policy
//! modifications. Events are delivered to userspace (auditd) via
//! a netlink socket for logging and analysis.

// ---------------------------------------------------------------------------
// Audit message types
// ---------------------------------------------------------------------------

/// Syscall event.
pub const AUDIT_SYSCALL: u16 = 1300;
/// File path.
pub const AUDIT_PATH: u16 = 1302;
/// IPC record.
pub const AUDIT_IPC: u16 = 1303;
/// Socket address.
pub const AUDIT_SOCKADDR: u16 = 1306;
/// CWD record.
pub const AUDIT_CWD: u16 = 1307;
/// execve arguments.
pub const AUDIT_EXECVE: u16 = 1309;
/// User login event.
pub const AUDIT_USER_LOGIN: u16 = 1112;
/// User logout event.
pub const AUDIT_USER_LOGOUT: u16 = 1113;
/// User auth event.
pub const AUDIT_USER_AUTH: u16 = 1100;
/// Configuration change.
pub const AUDIT_CONFIG_CHANGE: u16 = 1305;

// ---------------------------------------------------------------------------
// Audit arch (for syscall table identification)
// ---------------------------------------------------------------------------

/// x86_64.
pub const AUDIT_ARCH_X86_64: u32 = 0xC000003E;
/// i386.
pub const AUDIT_ARCH_I386: u32 = 0x40000003;
/// ARM (32-bit).
pub const AUDIT_ARCH_ARM: u32 = 0x40000028;
/// AArch64.
pub const AUDIT_ARCH_AARCH64: u32 = 0xC00000B7;
/// RISC-V 64.
pub const AUDIT_ARCH_RISCV64: u32 = 0xC00000F3;

// ---------------------------------------------------------------------------
// Audit filter types
// ---------------------------------------------------------------------------

/// Filter on task creation.
pub const AUDIT_FILTER_TASK: u32 = 0;
/// Filter on syscall entry.
pub const AUDIT_FILTER_ENTRY: u32 = 1;
/// Filter on syscall exit.
pub const AUDIT_FILTER_EXIT: u32 = 2;
/// Filter on user messages.
pub const AUDIT_FILTER_USER: u32 = 3;
/// Filter on filesystem events.
pub const AUDIT_FILTER_FS: u32 = 5;

// ---------------------------------------------------------------------------
// Audit rule fields
// ---------------------------------------------------------------------------

/// Process ID.
pub const AUDIT_PID: u32 = 0;
/// User ID.
pub const AUDIT_UID: u32 = 1;
/// Effective UID.
pub const AUDIT_EUID: u32 = 2;
/// Group ID.
pub const AUDIT_GID: u32 = 5;
/// Syscall number.
pub const AUDIT_MSGTYPE: u32 = 12;
/// Architecture.
pub const AUDIT_ARCH: u32 = 11;
/// Object user.
pub const AUDIT_OBJ_USER: u32 = 21;
/// Exit value.
pub const AUDIT_EXIT: u32 = 103;
/// Success/failure.
pub const AUDIT_SUCCESS: u32 = 104;

// ---------------------------------------------------------------------------
// Audit actions
// ---------------------------------------------------------------------------

/// Always generate record.
pub const AUDIT_ALWAYS: u32 = 2;
/// Never generate record.
pub const AUDIT_NEVER: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            AUDIT_SYSCALL, AUDIT_PATH, AUDIT_IPC, AUDIT_SOCKADDR,
            AUDIT_CWD, AUDIT_EXECVE, AUDIT_USER_LOGIN,
            AUDIT_USER_LOGOUT, AUDIT_USER_AUTH, AUDIT_CONFIG_CHANGE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_arch_values_distinct() {
        let arches = [
            AUDIT_ARCH_X86_64, AUDIT_ARCH_I386, AUDIT_ARCH_ARM,
            AUDIT_ARCH_AARCH64, AUDIT_ARCH_RISCV64,
        ];
        for i in 0..arches.len() {
            for j in (i + 1)..arches.len() {
                assert_ne!(arches[i], arches[j]);
            }
        }
    }

    #[test]
    fn test_filter_types_distinct() {
        let filters = [
            AUDIT_FILTER_TASK, AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_EXIT, AUDIT_FILTER_USER, AUDIT_FILTER_FS,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        assert_ne!(AUDIT_ALWAYS, AUDIT_NEVER);
    }
}
