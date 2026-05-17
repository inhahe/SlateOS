//! `<linux/nmi.h>` — Non-Maskable Interrupt (NMI) constants.
//!
//! NMIs cannot be disabled by software and are used for critical
//! system functions: hardware watchdogs (detect hard lockups), debug
//! break (SysRq-L dumps all CPUs), performance monitoring overflow,
//! and platform-specific critical events (memory errors, thermal
//! alerts). The NMI handler must be extremely careful: it can
//! preempt anything including other interrupt handlers, so it cannot
//! use any locking or sleeping primitives.

// ---------------------------------------------------------------------------
// NMI types/reasons
// ---------------------------------------------------------------------------

/// Unknown NMI source.
pub const NMI_UNKNOWN: u32 = 0;
/// Local APIC hardware watchdog.
pub const NMI_LOCAL: u32 = 1;
/// Performance monitoring counter overflow.
pub const NMI_PERF: u32 = 2;
/// I/O check (memory parity error, legacy).
pub const NMI_IO_CHECK: u32 = 3;
/// External NMI (physical button, IPMI, debug).
pub const NMI_EXTERNAL: u32 = 4;
/// Software-generated NMI (IPI for debug).
pub const NMI_SOFTWARE: u32 = 5;
/// Machine check exception delivered as NMI.
pub const NMI_MCE: u32 = 6;

// ---------------------------------------------------------------------------
// NMI watchdog modes
// ---------------------------------------------------------------------------

/// Watchdog disabled.
pub const NMI_WATCHDOG_DISABLED: u32 = 0;
/// Watchdog using perf event (PMU-based).
pub const NMI_WATCHDOG_PERF: u32 = 1;
/// Watchdog using HPET (fallback).
pub const NMI_WATCHDOG_HPET: u32 = 2;

// ---------------------------------------------------------------------------
// NMI handler return values
// ---------------------------------------------------------------------------

/// NMI was not handled (pass to next handler).
pub const NMI_DONE: u32 = 0;
/// NMI was handled (stop calling other handlers).
pub const NMI_HANDLED: u32 = 1;

// ---------------------------------------------------------------------------
// Hard lockup detection thresholds
// ---------------------------------------------------------------------------

/// Default watchdog threshold in seconds.
pub const WATCHDOG_THRESH_DEFAULT: u32 = 10;
/// Minimum watchdog threshold.
pub const WATCHDOG_THRESH_MIN: u32 = 1;
/// Maximum watchdog threshold.
pub const WATCHDOG_THRESH_MAX: u32 = 60;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nmi_types_distinct() {
        let types = [
            NMI_UNKNOWN, NMI_LOCAL, NMI_PERF, NMI_IO_CHECK,
            NMI_EXTERNAL, NMI_SOFTWARE, NMI_MCE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_watchdog_modes_distinct() {
        let modes = [NMI_WATCHDOG_DISABLED, NMI_WATCHDOG_PERF, NMI_WATCHDOG_HPET];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_handler_returns_distinct() {
        assert_ne!(NMI_DONE, NMI_HANDLED);
    }

    #[test]
    fn test_watchdog_thresholds() {
        assert!(WATCHDOG_THRESH_MIN < WATCHDOG_THRESH_DEFAULT);
        assert!(WATCHDOG_THRESH_DEFAULT < WATCHDOG_THRESH_MAX);
    }
}
