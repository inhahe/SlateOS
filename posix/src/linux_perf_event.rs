//! `<linux/perf_event.h>` — performance monitoring events.
//!
//! Provides types and constants for the `perf_event_open()` syscall
//! and the hardware/software performance counters.

use crate::errno;

// ---------------------------------------------------------------------------
// perf_type_id — what is being measured
// ---------------------------------------------------------------------------

/// Hardware event.
pub const PERF_TYPE_HARDWARE: u32 = 0;
/// Software event.
pub const PERF_TYPE_SOFTWARE: u32 = 1;
/// Tracepoint event.
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
/// Raw hardware cache event.
pub const PERF_TYPE_HW_CACHE: u32 = 3;
/// Raw event (CPU-specific).
pub const PERF_TYPE_RAW: u32 = 4;
/// Breakpoint event.
pub const PERF_TYPE_BREAKPOINT: u32 = 5;

// ---------------------------------------------------------------------------
// perf_hw_id — hardware events
// ---------------------------------------------------------------------------

/// Total CPU cycles.
pub const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
/// Retired instructions.
pub const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
/// Cache accesses.
pub const PERF_COUNT_HW_CACHE_REFERENCES: u64 = 2;
/// Cache misses.
pub const PERF_COUNT_HW_CACHE_MISSES: u64 = 3;
/// Retired branch instructions.
pub const PERF_COUNT_HW_BRANCH_INSTRUCTIONS: u64 = 4;
/// Mispredicted branch instructions.
pub const PERF_COUNT_HW_BRANCH_MISSES: u64 = 5;
/// Bus cycles.
pub const PERF_COUNT_HW_BUS_CYCLES: u64 = 6;
/// Stalled frontend cycles.
pub const PERF_COUNT_HW_STALLED_CYCLES_FRONTEND: u64 = 7;
/// Stalled backend cycles.
pub const PERF_COUNT_HW_STALLED_CYCLES_BACKEND: u64 = 8;
/// Total reference cycles (not affected by frequency scaling).
pub const PERF_COUNT_HW_REF_CPU_CYCLES: u64 = 9;

// ---------------------------------------------------------------------------
// perf_sw_ids — software events
// ---------------------------------------------------------------------------

/// CPU clock.
pub const PERF_COUNT_SW_CPU_CLOCK: u64 = 0;
/// Task clock.
pub const PERF_COUNT_SW_TASK_CLOCK: u64 = 1;
/// Page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS: u64 = 2;
/// Context switches.
pub const PERF_COUNT_SW_CONTEXT_SWITCHES: u64 = 3;
/// CPU migrations.
pub const PERF_COUNT_SW_CPU_MIGRATIONS: u64 = 4;
/// Minor page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MIN: u64 = 5;
/// Major page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MAJ: u64 = 6;
/// Alignment faults.
pub const PERF_COUNT_SW_ALIGNMENT_FAULTS: u64 = 7;
/// Emulation faults.
pub const PERF_COUNT_SW_EMULATION_FAULTS: u64 = 8;

// ---------------------------------------------------------------------------
// perf_event_open flags
// ---------------------------------------------------------------------------

/// Count in disabled state.
pub const PERF_FLAG_FD_NO_GROUP: u64 = 1;
/// Write event on every overflow.
pub const PERF_FLAG_FD_OUTPUT: u64 = 2;
/// Create a PID cgroup event.
pub const PERF_FLAG_PID_CGROUP: u64 = 4;
/// Close-on-exec for the returned fd.
pub const PERF_FLAG_FD_CLOEXEC: u64 = 8;

// ---------------------------------------------------------------------------
// PERF_EVENT_IOC_* ioctl commands
// ---------------------------------------------------------------------------

/// Enable a perf event.
pub const PERF_EVENT_IOC_ENABLE: u64 = 0x2400;
/// Disable a perf event.
pub const PERF_EVENT_IOC_DISABLE: u64 = 0x2401;
/// Refresh overflow count.
pub const PERF_EVENT_IOC_REFRESH: u64 = 0x2402;
/// Reset counter values.
pub const PERF_EVENT_IOC_RESET: u64 = 0x2403;
/// Set output.
pub const PERF_EVENT_IOC_SET_OUTPUT: u64 = 0x2405;
/// Set BPF program.
pub const PERF_EVENT_IOC_SET_BPF: u64 = 0x2408;

// ---------------------------------------------------------------------------
// PerfEventAttr — describes what to measure (simplified)
// ---------------------------------------------------------------------------

/// Simplified perf event attributes.
///
/// The full Linux `perf_event_attr` is ~120 bytes with many bitfields.
/// This provides the essential fields for common use cases.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PerfEventAttr {
    /// Event type (PERF_TYPE_*).
    pub type_: u32,
    /// Size of this struct.
    pub size: u32,
    /// Type-specific config (PERF_COUNT_HW_* or PERF_COUNT_SW_*).
    pub config: u64,
    /// Sample period or frequency.
    pub sample_period_or_freq: u64,
    /// Sample type bitmask.
    pub sample_type: u64,
    /// Read format bitmask.
    pub read_format: u64,
    /// Bitfield flags (disabled, inherit, pinned, exclusive, etc.).
    pub flags: u64,
    /// Wakeup events/watermark.
    pub wakeup_events_or_watermark: u32,
    /// Breakpoint type.
    pub bp_type: u32,
    /// Breakpoint address or config1.
    pub bp_addr_or_config1: u64,
    /// Breakpoint len or config2.
    pub bp_len_or_config2: u64,
    /// Branch sample type.
    pub branch_sample_type: u64,
    /// Sample registers (user).
    pub sample_regs_user: u64,
    /// Sample stack (user).
    pub sample_stack_user: u32,
    /// Clock ID.
    pub clockid: i32,
    /// Sample registers (intr).
    pub sample_regs_intr: u64,
    /// Aux watermark.
    pub aux_watermark: u32,
    /// Sample max stack.
    pub sample_max_stack: u16,
    /// Reserved.
    pub _reserved_2: u16,
}

impl PerfEventAttr {
    /// Create a zeroed `PerfEventAttr` with `size` set correctly.
    pub fn new() -> Self {
        let mut attr: Self = unsafe { core::mem::zeroed() };
        attr.size = core::mem::size_of::<Self>() as u32;
        attr
    }
}

// ---------------------------------------------------------------------------
// perf_event_open syscall
// ---------------------------------------------------------------------------

/// Open a performance monitoring event.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn perf_event_open(
    _attr: *mut PerfEventAttr,
    _pid: i32,
    _cpu: i32,
    _group_fd: i32,
    _flags: u64,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_types_distinct() {
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
    fn test_hw_events_sequential() {
        assert_eq!(PERF_COUNT_HW_CPU_CYCLES, 0);
        assert_eq!(PERF_COUNT_HW_INSTRUCTIONS, 1);
        assert_eq!(PERF_COUNT_HW_REF_CPU_CYCLES, 9);
    }

    #[test]
    fn test_sw_events_sequential() {
        assert_eq!(PERF_COUNT_SW_CPU_CLOCK, 0);
        assert_eq!(PERF_COUNT_SW_EMULATION_FAULTS, 8);
    }

    #[test]
    fn test_perf_event_attr_new() {
        let attr = PerfEventAttr::new();
        assert_eq!(attr.type_, 0);
        assert_eq!(attr.size as usize, core::mem::size_of::<PerfEventAttr>());
        assert_eq!(attr.config, 0);
    }

    #[test]
    fn test_flags_are_bits() {
        let combined = PERF_FLAG_FD_NO_GROUP | PERF_FLAG_FD_OUTPUT
            | PERF_FLAG_PID_CGROUP | PERF_FLAG_FD_CLOEXEC;
        assert_eq!(combined, 0x0F);
    }

    #[test]
    fn test_ioc_commands_distinct() {
        let cmds = [
            PERF_EVENT_IOC_ENABLE, PERF_EVENT_IOC_DISABLE,
            PERF_EVENT_IOC_REFRESH, PERF_EVENT_IOC_RESET,
            PERF_EVENT_IOC_SET_OUTPUT, PERF_EVENT_IOC_SET_BPF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_perf_event_open_stub() {
        let mut attr = PerfEventAttr::new();
        assert_eq!(perf_event_open(&mut attr, -1, 0, -1, 0), -1);
    }
}
