//! `<linux/kobject.h>` — Kernel object model constants.
//!
//! kobjects are the foundation of the Linux device model. Every
//! device, driver, bus, and class is represented by a kobject
//! that provides sysfs representation, reference counting, and
//! uevent notification. ksets group related kobjects.

// ---------------------------------------------------------------------------
// Uevent actions
// ---------------------------------------------------------------------------

/// Device added.
pub const KOBJ_ADD: u32 = 0;
/// Device removed.
pub const KOBJ_REMOVE: u32 = 1;
/// Device changed.
pub const KOBJ_CHANGE: u32 = 2;
/// Device moved.
pub const KOBJ_MOVE: u32 = 3;
/// Device online.
pub const KOBJ_ONLINE: u32 = 4;
/// Device offline.
pub const KOBJ_OFFLINE: u32 = 5;
/// Device bound to driver.
pub const KOBJ_BIND: u32 = 6;
/// Device unbound from driver.
pub const KOBJ_UNBIND: u32 = 7;

// ---------------------------------------------------------------------------
// Uevent action strings (for matching)
// ---------------------------------------------------------------------------

/// "add"
pub const KOBJ_ACTION_ADD: &str = "add";
/// "remove"
pub const KOBJ_ACTION_REMOVE: &str = "remove";
/// "change"
pub const KOBJ_ACTION_CHANGE: &str = "change";
/// "move"
pub const KOBJ_ACTION_MOVE: &str = "move";
/// "online"
pub const KOBJ_ACTION_ONLINE: &str = "online";
/// "offline"
pub const KOBJ_ACTION_OFFLINE: &str = "offline";
/// "bind"
pub const KOBJ_ACTION_BIND: &str = "bind";
/// "unbind"
pub const KOBJ_ACTION_UNBIND: &str = "unbind";

// ---------------------------------------------------------------------------
// Uevent environment variable names
// ---------------------------------------------------------------------------

/// Action environment variable.
pub const KOBJ_UEVENT_ACTION: &str = "ACTION";
/// Device path environment variable.
pub const KOBJ_UEVENT_DEVPATH: &str = "DEVPATH";
/// Subsystem environment variable.
pub const KOBJ_UEVENT_SUBSYSTEM: &str = "SUBSYSTEM";
/// Sequence number.
pub const KOBJ_UEVENT_SEQNUM: &str = "SEQNUM";

// ---------------------------------------------------------------------------
// Uevent buffer limits
// ---------------------------------------------------------------------------

/// Maximum uevent buffer size.
pub const UEVENT_BUFFER_SIZE: usize = 2048;
/// Maximum number of uevent environment variables.
pub const UEVENT_NUM_ENVP: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            KOBJ_ADD, KOBJ_REMOVE, KOBJ_CHANGE, KOBJ_MOVE,
            KOBJ_ONLINE, KOBJ_OFFLINE, KOBJ_BIND, KOBJ_UNBIND,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_action_strings_distinct() {
        let strings = [
            KOBJ_ACTION_ADD, KOBJ_ACTION_REMOVE, KOBJ_ACTION_CHANGE,
            KOBJ_ACTION_MOVE, KOBJ_ACTION_ONLINE, KOBJ_ACTION_OFFLINE,
            KOBJ_ACTION_BIND, KOBJ_ACTION_UNBIND,
        ];
        for i in 0..strings.len() {
            for j in (i + 1)..strings.len() {
                assert_ne!(strings[i], strings[j]);
            }
        }
    }

    #[test]
    fn test_uevent_env_vars_distinct() {
        let vars = [
            KOBJ_UEVENT_ACTION, KOBJ_UEVENT_DEVPATH,
            KOBJ_UEVENT_SUBSYSTEM, KOBJ_UEVENT_SEQNUM,
        ];
        for i in 0..vars.len() {
            for j in (i + 1)..vars.len() {
                assert_ne!(vars[i], vars[j]);
            }
        }
    }

    #[test]
    fn test_buffer_limits() {
        assert_eq!(UEVENT_BUFFER_SIZE, 2048);
        assert_eq!(UEVENT_NUM_ENVP, 64);
    }
}
