//! `<linux/nmi.h>` — Non-Maskable Interrupt watchdog ABI.
//!
//! Linux runs a hardlockup watchdog driven by NMIs to catch CPUs
//! that are stuck with interrupts disabled — the kind of hang that
//! a normal timer-driven softlockup detector cannot see. Userspace
//! tunes it through `/proc/sys/kernel/*` and triggers a deliberate
//! crash via `/proc/sysrq-trigger`.

// ---------------------------------------------------------------------------
// sysctl paths
// ---------------------------------------------------------------------------

pub const SYSCTL_NMI_WATCHDOG: &str = "/proc/sys/kernel/nmi_watchdog";
pub const SYSCTL_HARDLOCKUP_PANIC: &str = "/proc/sys/kernel/hardlockup_panic";
pub const SYSCTL_SOFTLOCKUP_PANIC: &str = "/proc/sys/kernel/softlockup_panic";
pub const SYSCTL_WATCHDOG: &str = "/proc/sys/kernel/watchdog";
pub const SYSCTL_WATCHDOG_THRESH: &str = "/proc/sys/kernel/watchdog_thresh";
pub const SYSCTL_WATCHDOG_CPUMASK: &str = "/proc/sys/kernel/watchdog_cpumask";
pub const SYSCTL_PANIC_ON_OOPS: &str = "/proc/sys/kernel/panic_on_oops";
pub const SYSCTL_PANIC_ON_UNRECOVERED_NMI: &str =
    "/proc/sys/kernel/panic_on_unrecovered_nmi";
pub const SYSCTL_UNKNOWN_NMI_PANIC: &str = "/proc/sys/kernel/unknown_nmi_panic";

// ---------------------------------------------------------------------------
// Tunable defaults and bounds
// ---------------------------------------------------------------------------

/// Default watchdog timeout (`watchdog_thresh`) in seconds.
pub const WATCHDOG_DEFAULT_THRESH: u32 = 10;
/// Maximum value the kernel accepts for `watchdog_thresh`.
pub const WATCHDOG_MAX_THRESH: u32 = 60;
/// Minimum value (0 disables the watchdog entirely).
pub const WATCHDOG_MIN_THRESH: u32 = 0;

/// Soft-lockup detection runs at 5x the hardlockup rate.
pub const SOFTLOCKUP_RESET_RATIO: u32 = 5;

// ---------------------------------------------------------------------------
// SysRq trigger
// ---------------------------------------------------------------------------

pub const SYSRQ_TRIGGER_PATH: &str = "/proc/sysrq-trigger";
/// `c` — crash the kernel via a NULL deref. Used to force a kdump.
pub const SYSRQ_CRASH: char = 'c';
/// `t` — dump all task state via NMI backtraces.
pub const SYSRQ_SHOW_TASKS: char = 't';
/// `l` — show backtrace for all active CPUs (uses NMI).
pub const SYSRQ_SHOW_ALL_CPU_BT: char = 'l';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysctl_paths_under_kernel() {
        let p = [
            SYSCTL_NMI_WATCHDOG,
            SYSCTL_HARDLOCKUP_PANIC,
            SYSCTL_SOFTLOCKUP_PANIC,
            SYSCTL_WATCHDOG,
            SYSCTL_WATCHDOG_THRESH,
            SYSCTL_WATCHDOG_CPUMASK,
            SYSCTL_PANIC_ON_OOPS,
            SYSCTL_PANIC_ON_UNRECOVERED_NMI,
            SYSCTL_UNKNOWN_NMI_PANIC,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/kernel/"));
        }
    }

    #[test]
    fn test_thresh_bounds_ordered() {
        assert!(WATCHDOG_MIN_THRESH <= WATCHDOG_DEFAULT_THRESH);
        assert!(WATCHDOG_DEFAULT_THRESH <= WATCHDOG_MAX_THRESH);
        assert_eq!(WATCHDOG_DEFAULT_THRESH, 10);
        assert_eq!(WATCHDOG_MAX_THRESH, 60);
    }

    #[test]
    fn test_softlockup_ratio() {
        // Softlockup detector wakes 5x more often than the hardlockup one.
        assert_eq!(SOFTLOCKUP_RESET_RATIO, 5);
    }

    #[test]
    fn test_sysrq_keys_distinct() {
        assert_ne!(SYSRQ_CRASH, SYSRQ_SHOW_TASKS);
        assert_ne!(SYSRQ_CRASH, SYSRQ_SHOW_ALL_CPU_BT);
        assert_ne!(SYSRQ_SHOW_TASKS, SYSRQ_SHOW_ALL_CPU_BT);
        assert_eq!(SYSRQ_TRIGGER_PATH, "/proc/sysrq-trigger");
    }
}
