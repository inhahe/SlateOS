//! `<linux/safesetid.h>` — SafeSetID LSM constants.
//!
//! SafeSetID is a Linux Security Module that restricts which UIDs/GIDs
//! a process can transition to via setuid/setgid/setgroups. Unlike
//! traditional Unix where any root process can become any UID, SafeSetID
//! requires explicit allow-list rules for UID/GID transitions. This
//! provides defense-in-depth for systems that need fine-grained control
//! over identity transitions, such as ChromeOS and container runtimes.

// ---------------------------------------------------------------------------
// SafeSetID policy rule types
// ---------------------------------------------------------------------------

/// UID transition rule (source_uid → target_uid).
pub const SAFESETID_RULE_UID: u32 = 0;
/// GID transition rule (source_gid → target_gid).
pub const SAFESETID_RULE_GID: u32 = 1;

// ---------------------------------------------------------------------------
// SafeSetID policy actions
// ---------------------------------------------------------------------------

/// Allow the transition.
pub const SAFESETID_ACTION_ALLOW: u32 = 0;
/// Deny the transition.
pub const SAFESETID_ACTION_DENY: u32 = 1;
/// Log the transition (permissive mode).
pub const SAFESETID_ACTION_LOG: u32 = 2;

// ---------------------------------------------------------------------------
// SafeSetID special UIDs
// ---------------------------------------------------------------------------

/// Wildcard UID (match any source/target).
pub const SAFESETID_UID_WILDCARD: u32 = 0xFFFF_FFFF;
/// No UID set (uninitialized).
pub const SAFESETID_UID_NONE: u32 = 0xFFFF_FFFE;

// ---------------------------------------------------------------------------
// SafeSetID policy states
// ---------------------------------------------------------------------------

/// Policy not loaded (module inactive).
pub const SAFESETID_POLICY_NONE: u32 = 0;
/// Policy loaded and enforcing.
pub const SAFESETID_POLICY_ACTIVE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_types_distinct() {
        assert_ne!(SAFESETID_RULE_UID, SAFESETID_RULE_GID);
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [
            SAFESETID_ACTION_ALLOW, SAFESETID_ACTION_DENY,
            SAFESETID_ACTION_LOG,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_special_uids() {
        assert_ne!(SAFESETID_UID_WILDCARD, SAFESETID_UID_NONE);
        // Neither should be a normal UID like 0 (root)
        assert_ne!(SAFESETID_UID_WILDCARD, 0);
        assert_ne!(SAFESETID_UID_NONE, 0);
    }

    #[test]
    fn test_policy_states_distinct() {
        assert_ne!(SAFESETID_POLICY_NONE, SAFESETID_POLICY_ACTIVE);
    }
}
