//! Kernel panic tunables (`<linux/panic.h>` + `kernel/panic.c` sysctls).
//!
//! When the kernel panics it can either reboot, hang for an operator,
//! or kexec a crashdump. The sysctls below are what `systemd`,
//! `kdump-tools`, and embedded init systems poke to choose the policy.

// ---------------------------------------------------------------------------
// sysctl paths
// ---------------------------------------------------------------------------

pub const SYSCTL_PANIC: &str = "/proc/sys/kernel/panic";
pub const SYSCTL_PANIC_ON_OOPS: &str = "/proc/sys/kernel/panic_on_oops";
pub const SYSCTL_PANIC_ON_WARN: &str = "/proc/sys/kernel/panic_on_warn";
pub const SYSCTL_PANIC_PRINT: &str = "/proc/sys/kernel/panic_print";
pub const SYSCTL_OOPS_LIMIT: &str = "/proc/sys/kernel/oops_limit";
pub const SYSCTL_WARN_LIMIT: &str = "/proc/sys/kernel/warn_limit";
pub const SYSCTL_HUNG_TASK_PANIC: &str = "/proc/sys/kernel/hung_task_panic";
pub const SYSCTL_HUNG_TASK_TIMEOUT_SECS: &str =
    "/proc/sys/kernel/hung_task_timeout_secs";

// ---------------------------------------------------------------------------
// `panic` timeout encoding
// ---------------------------------------------------------------------------

/// `panic = 0` — wait forever, useful for kdump + manual recovery.
pub const PANIC_TIMEOUT_FOREVER: i32 = 0;
/// `panic = -1` — reboot immediately (no delay).
pub const PANIC_TIMEOUT_IMMEDIATE: i32 = -1;

// ---------------------------------------------------------------------------
// `panic_print` bitmask — extra info dumped on panic
// ---------------------------------------------------------------------------

pub const PANIC_PRINT_ALL_TASKS_INFO: u32 = 1 << 0;
pub const PANIC_PRINT_SYSTEM_MEMORY_INFO: u32 = 1 << 1;
pub const PANIC_PRINT_TIMER_INFO: u32 = 1 << 2;
pub const PANIC_PRINT_LOCK_INFO: u32 = 1 << 3;
pub const PANIC_PRINT_FTRACE_INFO: u32 = 1 << 4;
pub const PANIC_PRINT_ALL_PRINTK_MSG: u32 = 1 << 5;
pub const PANIC_PRINT_ALL_CPU_BT: u32 = 1 << 6;
pub const PANIC_PRINT_BLOCKED_TASKS: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

pub const HUNG_TASK_TIMEOUT_DEFAULT_SECS: u32 = 120;
pub const HUNG_TASK_TIMEOUT_MAX_SECS: u32 = 1200;

/// Default `oops_limit` shipped since 6.2: after this many oopses the
/// kernel panics so an attacker can't keep churning races.
pub const OOPS_LIMIT_DEFAULT: u32 = 10_000;
pub const WARN_LIMIT_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysctl_paths_under_kernel() {
        let p = [
            SYSCTL_PANIC,
            SYSCTL_PANIC_ON_OOPS,
            SYSCTL_PANIC_ON_WARN,
            SYSCTL_PANIC_PRINT,
            SYSCTL_OOPS_LIMIT,
            SYSCTL_WARN_LIMIT,
            SYSCTL_HUNG_TASK_PANIC,
            SYSCTL_HUNG_TASK_TIMEOUT_SECS,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/kernel/"));
        }
    }

    #[test]
    fn test_panic_timeout_sentinels() {
        // The two magic values that flip "wait forever" vs "reboot now".
        assert_eq!(PANIC_TIMEOUT_FOREVER, 0);
        assert_eq!(PANIC_TIMEOUT_IMMEDIATE, -1);
    }

    #[test]
    fn test_panic_print_bits_dense_0_to_7() {
        let p = [
            PANIC_PRINT_ALL_TASKS_INFO,
            PANIC_PRINT_SYSTEM_MEMORY_INFO,
            PANIC_PRINT_TIMER_INFO,
            PANIC_PRINT_LOCK_INFO,
            PANIC_PRINT_FTRACE_INFO,
            PANIC_PRINT_ALL_PRINTK_MSG,
            PANIC_PRINT_ALL_CPU_BT,
            PANIC_PRINT_BLOCKED_TASKS,
        ];
        let mut or = 0u32;
        for (i, &v) in p.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
            or |= v;
        }
        // All eight bits packed in the low byte.
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_hung_task_defaults_in_range() {
        assert!(HUNG_TASK_TIMEOUT_DEFAULT_SECS <= HUNG_TASK_TIMEOUT_MAX_SECS);
        assert_eq!(HUNG_TASK_TIMEOUT_DEFAULT_SECS, 120);
        assert_eq!(HUNG_TASK_TIMEOUT_MAX_SECS, 1200);
    }

    #[test]
    fn test_oops_limit_default_nonzero() {
        // 0 would disable the cap; default is 10000 (since 6.2).
        assert_eq!(OOPS_LIMIT_DEFAULT, 10_000);
        // warn_limit ships at 0 (disabled by default).
        assert_eq!(WARN_LIMIT_DEFAULT, 0);
    }
}
