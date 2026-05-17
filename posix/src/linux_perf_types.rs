//! `<linux/perf_event.h>` — perf_event_open() constants.
//!
//! The perf subsystem provides hardware and software performance
//! counters, tracepoints, kprobes, uprobes, and more through a
//! unified interface. perf_event_open() creates a file descriptor
//! representing a performance counter or event source, which can
//! then be read, mmap'd (for ring buffer), or sampled.

// ---------------------------------------------------------------------------
// perf_type_id — event source types
// ---------------------------------------------------------------------------

/// Hardware event (CPU PMC).
pub const PERF_TYPE_HARDWARE: u32 = 0;
/// Software event (kernel counter).
pub const PERF_TYPE_SOFTWARE: u32 = 1;
/// Tracepoint event.
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
/// Raw hardware event (PMC encoding).
pub const PERF_TYPE_HW_CACHE: u32 = 3;
/// Raw PMU event.
pub const PERF_TYPE_RAW: u32 = 4;
/// Breakpoint event.
pub const PERF_TYPE_BREAKPOINT: u32 = 5;

// ---------------------------------------------------------------------------
// perf_hw_id — generic hardware events
// ---------------------------------------------------------------------------

/// Total CPU cycles.
pub const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
/// Retired instructions.
pub const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
/// Cache references.
pub const PERF_COUNT_HW_CACHE_REFERENCES: u64 = 2;
/// Cache misses.
pub const PERF_COUNT_HW_CACHE_MISSES: u64 = 3;
/// Branch instructions.
pub const PERF_COUNT_HW_BRANCH_INSTRUCTIONS: u64 = 4;
/// Branch mispredictions.
pub const PERF_COUNT_HW_BRANCH_MISSES: u64 = 5;
/// Bus cycles.
pub const PERF_COUNT_HW_BUS_CYCLES: u64 = 6;
/// Stalled cycles (frontend).
pub const PERF_COUNT_HW_STALLED_CYCLES_FRONTEND: u64 = 7;
/// Stalled cycles (backend).
pub const PERF_COUNT_HW_STALLED_CYCLES_BACKEND: u64 = 8;
/// Reference CPU cycles (unaffected by frequency scaling).
pub const PERF_COUNT_HW_REF_CPU_CYCLES: u64 = 9;

// ---------------------------------------------------------------------------
// perf_sw_ids — generic software events
// ---------------------------------------------------------------------------

/// Context switches.
pub const PERF_COUNT_SW_CONTEXT_SWITCHES: u64 = 3;
/// CPU migrations.
pub const PERF_COUNT_SW_CPU_MIGRATIONS: u64 = 4;
/// Page faults (minor + major).
pub const PERF_COUNT_SW_PAGE_FAULTS: u64 = 2;
/// Minor page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MIN: u64 = 5;
/// Major page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MAJ: u64 = 6;
/// Alignment faults.
pub const PERF_COUNT_SW_ALIGNMENT_FAULTS: u64 = 7;
/// Emulation faults.
pub const PERF_COUNT_SW_EMULATION_FAULTS: u64 = 8;
/// Task clock (nanoseconds of CPU time).
pub const PERF_COUNT_SW_TASK_CLOCK: u64 = 1;
/// CPU clock.
pub const PERF_COUNT_SW_CPU_CLOCK: u64 = 0;

// ---------------------------------------------------------------------------
// perf_event_attr flags
// ---------------------------------------------------------------------------

/// Count events only when process is on-CPU.
pub const PERF_ATTR_FLAG_DISABLED: u64 = 1 << 0;
/// Enable event automatically on exec.
pub const PERF_ATTR_FLAG_INHERIT: u64 = 1 << 1;
/// Exclude events from user mode.
pub const PERF_ATTR_FLAG_EXCLUDE_USER: u64 = 1 << 2;
/// Exclude events from kernel mode.
pub const PERF_ATTR_FLAG_EXCLUDE_KERNEL: u64 = 1 << 3;
/// Exclude events from hypervisor.
pub const PERF_ATTR_FLAG_EXCLUDE_HV: u64 = 1 << 4;
/// Exclude events from idle task.
pub const PERF_ATTR_FLAG_EXCLUDE_IDLE: u64 = 1 << 5;

// ---------------------------------------------------------------------------
// perf_event_ioc — ioctl commands
// ---------------------------------------------------------------------------

/// Enable the counter.
pub const PERF_EVENT_IOC_ENABLE: u32 = 0x2400;
/// Disable the counter.
pub const PERF_EVENT_IOC_DISABLE: u32 = 0x2401;
/// Reset counter to zero.
pub const PERF_EVENT_IOC_RESET: u32 = 0x2403;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            PERF_TYPE_HARDWARE, PERF_TYPE_SOFTWARE, PERF_TYPE_TRACEPOINT,
            PERF_TYPE_HW_CACHE, PERF_TYPE_RAW, PERF_TYPE_BREAKPOINT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hw_events_distinct() {
        let events = [
            PERF_COUNT_HW_CPU_CYCLES, PERF_COUNT_HW_INSTRUCTIONS,
            PERF_COUNT_HW_CACHE_REFERENCES, PERF_COUNT_HW_CACHE_MISSES,
            PERF_COUNT_HW_BRANCH_INSTRUCTIONS, PERF_COUNT_HW_BRANCH_MISSES,
            PERF_COUNT_HW_BUS_CYCLES, PERF_COUNT_HW_STALLED_CYCLES_FRONTEND,
            PERF_COUNT_HW_STALLED_CYCLES_BACKEND, PERF_COUNT_HW_REF_CPU_CYCLES,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_sw_events_distinct() {
        let events = [
            PERF_COUNT_SW_CPU_CLOCK, PERF_COUNT_SW_TASK_CLOCK,
            PERF_COUNT_SW_PAGE_FAULTS, PERF_COUNT_SW_CONTEXT_SWITCHES,
            PERF_COUNT_SW_CPU_MIGRATIONS, PERF_COUNT_SW_PAGE_FAULTS_MIN,
            PERF_COUNT_SW_PAGE_FAULTS_MAJ, PERF_COUNT_SW_ALIGNMENT_FAULTS,
            PERF_COUNT_SW_EMULATION_FAULTS,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_attr_flags_no_overlap() {
        let flags = [
            PERF_ATTR_FLAG_DISABLED, PERF_ATTR_FLAG_INHERIT,
            PERF_ATTR_FLAG_EXCLUDE_USER, PERF_ATTR_FLAG_EXCLUDE_KERNEL,
            PERF_ATTR_FLAG_EXCLUDE_HV, PERF_ATTR_FLAG_EXCLUDE_IDLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioc_commands_distinct() {
        let cmds = [PERF_EVENT_IOC_ENABLE, PERF_EVENT_IOC_DISABLE, PERF_EVENT_IOC_RESET];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
