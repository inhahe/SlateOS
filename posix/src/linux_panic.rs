//! `<linux/panic.h>` — Kernel panic constants.
//!
//! A kernel panic is a fatal error from which the kernel cannot
//! recover. The panic subsystem handles crash reporting, watchdog
//! timeouts, and automatic reboot. These constants control panic
//! behavior via sysctl and kernel command line.

// ---------------------------------------------------------------------------
// Panic action (what to do on panic)
// ---------------------------------------------------------------------------

/// Halt (freeze, do nothing).
pub const PANIC_ACTION_HALT: i32 = 0;
/// Reboot after timeout seconds.
pub const PANIC_ACTION_REBOOT: i32 = 1;
/// Power off.
pub const PANIC_ACTION_POWEROFF: i32 = 2;

// ---------------------------------------------------------------------------
// Panic timeout
// ---------------------------------------------------------------------------

/// Default panic timeout (0 = wait forever).
pub const PANIC_TIMEOUT_DEFAULT: i32 = 0;

// ---------------------------------------------------------------------------
// Panic print flags (what to print on panic)
// ---------------------------------------------------------------------------

/// Print all tasks.
pub const PANIC_PRINT_TASK_INFO: u32 = 1 << 0;
/// Print memory info.
pub const PANIC_PRINT_MEM_INFO: u32 = 1 << 1;
/// Print timer info.
pub const PANIC_PRINT_TIMER_INFO: u32 = 1 << 2;
/// Print lock info.
pub const PANIC_PRINT_LOCK_INFO: u32 = 1 << 3;
/// Print ftrace buffer.
pub const PANIC_PRINT_FTRACE_INFO: u32 = 1 << 4;
/// Print all printk messages.
pub const PANIC_PRINT_ALL_PRINTK_MSG: u32 = 1 << 5;
/// Print all CPUs' backtraces.
pub const PANIC_PRINT_ALL_CPU_BT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Oops handling
// ---------------------------------------------------------------------------

/// Oops prints.
pub const OOPS_PRINT_REGS: u32 = 1 << 0;
/// Oops prints memory.
pub const OOPS_PRINT_BACKTRACE: u32 = 1 << 1;
/// Oops prints modules.
pub const OOPS_PRINT_MODULES: u32 = 1 << 2;
/// Oops prints process.
pub const OOPS_PRINT_PROCESS: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Panic on oops
// ---------------------------------------------------------------------------

/// Don't panic on oops (continue running).
pub const PANIC_ON_OOPS_OFF: i32 = 0;
/// Panic on oops.
pub const PANIC_ON_OOPS_ON: i32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            PANIC_ACTION_HALT, PANIC_ACTION_REBOOT,
            PANIC_ACTION_POWEROFF,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_default_timeout() {
        assert_eq!(PANIC_TIMEOUT_DEFAULT, 0);
    }

    #[test]
    fn test_print_flags_powers_of_two() {
        let flags = [
            PANIC_PRINT_TASK_INFO, PANIC_PRINT_MEM_INFO,
            PANIC_PRINT_TIMER_INFO, PANIC_PRINT_LOCK_INFO,
            PANIC_PRINT_FTRACE_INFO, PANIC_PRINT_ALL_PRINTK_MSG,
            PANIC_PRINT_ALL_CPU_BT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_print_flags_no_overlap() {
        let flags = [
            PANIC_PRINT_TASK_INFO, PANIC_PRINT_MEM_INFO,
            PANIC_PRINT_TIMER_INFO, PANIC_PRINT_LOCK_INFO,
            PANIC_PRINT_FTRACE_INFO, PANIC_PRINT_ALL_PRINTK_MSG,
            PANIC_PRINT_ALL_CPU_BT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_oops_flags_powers_of_two() {
        let flags = [
            OOPS_PRINT_REGS, OOPS_PRINT_BACKTRACE,
            OOPS_PRINT_MODULES, OOPS_PRINT_PROCESS,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_panic_on_oops() {
        assert_ne!(PANIC_ON_OOPS_OFF, PANIC_ON_OOPS_ON);
    }
}
