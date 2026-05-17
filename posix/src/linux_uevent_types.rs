//! `<linux/kobject.h>` (uevent subset) — Kernel uevent constants.
//!
//! Uevents are the kernel's mechanism for notifying userspace about
//! device and object state changes. When a device is added, removed,
//! or its state changes, the kernel sends a uevent via netlink socket
//! (KOBJECT_UEVENT family) or through /sys/…/uevent. udev (or systemd-udevd)
//! listens for these events and creates/removes device nodes, runs
//! rules, and triggers hotplug scripts.

// ---------------------------------------------------------------------------
// Uevent actions
// ---------------------------------------------------------------------------

/// Device added to the system.
pub const KOBJ_ACTION_ADD: u32 = 0;
/// Device removed from the system.
pub const KOBJ_ACTION_REMOVE: u32 = 1;
/// Device state changed.
pub const KOBJ_ACTION_CHANGE: u32 = 2;
/// Device moved to a new location in sysfs.
pub const KOBJ_ACTION_MOVE: u32 = 3;
/// Device is coming online.
pub const KOBJ_ACTION_ONLINE: u32 = 4;
/// Device is going offline.
pub const KOBJ_ACTION_OFFLINE: u32 = 5;
/// Device binding to a driver.
pub const KOBJ_ACTION_BIND: u32 = 6;
/// Device unbinding from a driver.
pub const KOBJ_ACTION_UNBIND: u32 = 7;

// ---------------------------------------------------------------------------
// Uevent environment variable keys (well-known keys)
// ---------------------------------------------------------------------------

/// Maximum uevent environment buffer size.
pub const UEVENT_BUFFER_SIZE: u32 = 2048;
/// Maximum number of environment variables per uevent.
pub const UEVENT_NUM_ENVP: u32 = 64;
/// Maximum length of a single environment variable string.
pub const UEVENT_ENV_KEY_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// Netlink uevent groups
// ---------------------------------------------------------------------------

/// Kernel uevent multicast group.
pub const UEVENT_NETLINK_GROUP_KERNEL: u32 = 1;

// ---------------------------------------------------------------------------
// Uevent suppress flags
// ---------------------------------------------------------------------------

/// Suppress uevent emission (device not yet ready).
pub const UEVENT_SUPPRESS: u32 = 0x01;
/// Force uevent emission even if normally suppressed.
pub const UEVENT_FORCE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            KOBJ_ACTION_ADD, KOBJ_ACTION_REMOVE, KOBJ_ACTION_CHANGE,
            KOBJ_ACTION_MOVE, KOBJ_ACTION_ONLINE, KOBJ_ACTION_OFFLINE,
            KOBJ_ACTION_BIND, KOBJ_ACTION_UNBIND,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_buffer_limits() {
        assert!(UEVENT_BUFFER_SIZE > 0);
        assert!(UEVENT_NUM_ENVP > 0);
        assert!(UEVENT_ENV_KEY_MAX > 0);
        assert!(UEVENT_BUFFER_SIZE > UEVENT_ENV_KEY_MAX);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(UEVENT_SUPPRESS & UEVENT_FORCE, 0);
        assert!(UEVENT_SUPPRESS.is_power_of_two());
        assert!(UEVENT_FORCE.is_power_of_two());
    }
}
