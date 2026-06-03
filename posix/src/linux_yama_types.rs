//! `<linux/yama.h>` — Yama LSM constants.
//!
//! Yama is a Linux Security Module that provides additional ptrace
//! restrictions beyond standard DAC permissions. By default, any
//! process owned by the same user can ptrace any other process of
//! that user. Yama restricts this to prevent attacks where a
//! compromised process attaches to another (e.g., SSH agent, browser)
//! to steal credentials or inject code. Yama's ptrace_scope controls
//! are the primary defense against such attacks on desktop Linux.

// ---------------------------------------------------------------------------
// Yama ptrace scope levels
// ---------------------------------------------------------------------------

/// Classic ptrace permissions (any process can trace same-uid).
pub const YAMA_SCOPE_CLASSIC: u32 = 0;
/// Restricted ptrace (only direct parent can trace child).
pub const YAMA_SCOPE_RESTRICTED: u32 = 1;
/// Admin-only ptrace (only CAP_SYS_PTRACE can trace).
pub const YAMA_SCOPE_ADMIN_ONLY: u32 = 2;
/// No ptrace (ptrace completely disabled).
pub const YAMA_SCOPE_NO_ATTACH: u32 = 3;

// ---------------------------------------------------------------------------
// Yama relationship types
// ---------------------------------------------------------------------------

/// No relationship (cannot trace).
pub const YAMA_RELATION_NONE: u32 = 0;
/// Direct parent-child relationship.
pub const YAMA_RELATION_PARENT: u32 = 1;
/// Explicitly granted via prctl(PR_SET_PTRACER).
pub const YAMA_RELATION_GRANTED: u32 = 2;
/// Process has CAP_SYS_PTRACE.
pub const YAMA_RELATION_PRIVILEGED: u32 = 3;

// ---------------------------------------------------------------------------
// Yama prctl values (PR_SET_PTRACER targets)
// ---------------------------------------------------------------------------

/// Allow any process to trace me.
pub const YAMA_PTRACER_ANY: u32 = 0xFFFF_FFFF;
/// Revoke all ptracer grants.
pub const YAMA_PTRACER_DISABLE: u32 = 0;

// ---------------------------------------------------------------------------
// Yama decision results
// ---------------------------------------------------------------------------

/// Access allowed.
pub const YAMA_RESULT_ALLOW: i32 = 0;
/// Access denied by Yama policy.
pub const YAMA_RESULT_DENY: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scopes_ordered() {
        assert!(YAMA_SCOPE_CLASSIC < YAMA_SCOPE_RESTRICTED);
        assert!(YAMA_SCOPE_RESTRICTED < YAMA_SCOPE_ADMIN_ONLY);
        assert!(YAMA_SCOPE_ADMIN_ONLY < YAMA_SCOPE_NO_ATTACH);
    }

    #[test]
    fn test_relations_distinct() {
        let relations = [
            YAMA_RELATION_NONE,
            YAMA_RELATION_PARENT,
            YAMA_RELATION_GRANTED,
            YAMA_RELATION_PRIVILEGED,
        ];
        for i in 0..relations.len() {
            for j in (i + 1)..relations.len() {
                assert_ne!(relations[i], relations[j]);
            }
        }
    }

    #[test]
    fn test_ptracer_values() {
        assert_ne!(YAMA_PTRACER_ANY, YAMA_PTRACER_DISABLE);
        // ANY should be a sentinel value
        assert_eq!(YAMA_PTRACER_ANY, u32::MAX);
    }

    #[test]
    fn test_results_distinct() {
        assert_ne!(YAMA_RESULT_ALLOW, YAMA_RESULT_DENY);
    }
}
