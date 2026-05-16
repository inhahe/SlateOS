//! `<linux/notifier.h>` — Notifier chain constants.
//!
//! Notifier chains are the kernel's publish-subscribe mechanism.
//! Subsystems register callback functions on a chain; when an
//! event occurs, all registered callbacks are invoked. Different
//! chain types offer different locking guarantees (atomic, blocking,
//! raw, SRCU).

// ---------------------------------------------------------------------------
// Notifier return values
// ---------------------------------------------------------------------------

/// Done — continue calling other notifiers.
pub const NOTIFY_DONE: i32 = 0x0000;
/// OK — callback handled it, continue.
pub const NOTIFY_OK: i32 = 0x0001;
/// Stop chain — don't call further notifiers.
pub const NOTIFY_STOP_MASK: i32 = 0x8000;
/// Bad — error, but continue.
pub const NOTIFY_BAD: i32 = NOTIFY_STOP_MASK | 0x0002;
/// Stop — handled, stop chain.
pub const NOTIFY_STOP: i32 = NOTIFY_STOP_MASK | NOTIFY_OK;

// ---------------------------------------------------------------------------
// Notifier priority constants
// ---------------------------------------------------------------------------

/// Lowest priority.
pub const NOTIFIER_PRIO_LOWEST: i32 = i32::MIN;
/// Default priority.
pub const NOTIFIER_PRIO_DEFAULT: i32 = 0;
/// Highest priority.
pub const NOTIFIER_PRIO_HIGHEST: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Common notifier chain events
// ---------------------------------------------------------------------------

// CPU events
/// CPU online.
pub const CPU_ONLINE: u32 = 0x0002;
/// CPU up prepare.
pub const CPU_UP_PREPARE: u32 = 0x0003;
/// CPU dead.
pub const CPU_DEAD: u32 = 0x0007;
/// CPU down prepare.
pub const CPU_DOWN_PREPARE: u32 = 0x0005;
/// CPU up canceled.
pub const CPU_UP_CANCELED: u32 = 0x0004;
/// CPU down failed.
pub const CPU_DOWN_FAILED: u32 = 0x0006;
/// CPU post-dead.
pub const CPU_POST_DEAD: u32 = 0x0009;

// Memory events
/// Memory going online.
pub const MEM_GOING_ONLINE: u32 = 0x0001;
/// Memory cancel online.
pub const MEM_CANCEL_ONLINE: u32 = 0x0002;
/// Memory online.
pub const MEM_ONLINE: u32 = 0x0003;
/// Memory going offline.
pub const MEM_GOING_OFFLINE: u32 = 0x0004;
/// Memory cancel offline.
pub const MEM_CANCEL_OFFLINE: u32 = 0x0005;
/// Memory offline.
pub const MEM_OFFLINE: u32 = 0x0006;

// Network events
/// Netdev up.
pub const NETDEV_UP: u32 = 0x0001;
/// Netdev down.
pub const NETDEV_DOWN: u32 = 0x0002;
/// Netdev reboot.
pub const NETDEV_REBOOT: u32 = 0x0003;
/// Netdev change.
pub const NETDEV_CHANGE: u32 = 0x0004;
/// Netdev register.
pub const NETDEV_REGISTER: u32 = 0x0005;
/// Netdev unregister.
pub const NETDEV_UNREGISTER: u32 = 0x0006;
/// Netdev change MTU.
pub const NETDEV_CHANGEMTU: u32 = 0x0007;
/// Netdev change address.
pub const NETDEV_CHANGEADDR: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_values() {
        assert_eq!(NOTIFY_DONE, 0);
        assert_eq!(NOTIFY_OK, 1);
    }

    #[test]
    fn test_stop_mask() {
        assert_ne!(NOTIFY_STOP_MASK & NOTIFY_BAD, 0);
        assert_ne!(NOTIFY_STOP_MASK & NOTIFY_STOP, 0);
        assert_eq!(NOTIFY_STOP_MASK & NOTIFY_DONE, 0);
        assert_eq!(NOTIFY_STOP_MASK & NOTIFY_OK, 0);
    }

    #[test]
    fn test_return_distinct() {
        let returns = [NOTIFY_DONE, NOTIFY_OK, NOTIFY_BAD, NOTIFY_STOP];
        for i in 0..returns.len() {
            for j in (i + 1)..returns.len() {
                assert_ne!(returns[i], returns[j]);
            }
        }
    }

    #[test]
    fn test_priorities() {
        assert!(NOTIFIER_PRIO_LOWEST < NOTIFIER_PRIO_DEFAULT);
        assert!(NOTIFIER_PRIO_DEFAULT < NOTIFIER_PRIO_HIGHEST);
    }

    #[test]
    fn test_cpu_events_distinct() {
        let events = [
            CPU_ONLINE, CPU_UP_PREPARE, CPU_DEAD,
            CPU_DOWN_PREPARE, CPU_UP_CANCELED,
            CPU_DOWN_FAILED, CPU_POST_DEAD,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_mem_events_distinct() {
        let events = [
            MEM_GOING_ONLINE, MEM_CANCEL_ONLINE, MEM_ONLINE,
            MEM_GOING_OFFLINE, MEM_CANCEL_OFFLINE, MEM_OFFLINE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_netdev_events_distinct() {
        let events = [
            NETDEV_UP, NETDEV_DOWN, NETDEV_REBOOT,
            NETDEV_CHANGE, NETDEV_REGISTER, NETDEV_UNREGISTER,
            NETDEV_CHANGEMTU, NETDEV_CHANGEADDR,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
