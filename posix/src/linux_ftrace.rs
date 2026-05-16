//! `<linux/ftrace.h>` — Function tracing constants.
//!
//! ftrace is the kernel's function-level tracer. It instruments
//! function entry/exit points (via compiler-inserted nops that
//! are patched at runtime) and provides various tracers:
//! function, function_graph, irqsoff, preemptoff, etc.

// ---------------------------------------------------------------------------
// Ftrace flags (per-function)
// ---------------------------------------------------------------------------

/// Function is traced.
pub const FTRACE_FL_ENABLED: u32 = 1 << 0;
/// Function uses regs (saves full register state).
pub const FTRACE_FL_REGS: u32 = 1 << 1;
/// Function uses regs_en (conditional regs).
pub const FTRACE_FL_REGS_EN: u32 = 1 << 2;
/// Function trampoline.
pub const FTRACE_FL_TRAMP: u32 = 1 << 3;
/// Function trampoline enabled.
pub const FTRACE_FL_TRAMP_EN: u32 = 1 << 4;
/// IP modified (detour).
pub const FTRACE_FL_IPMODIFY: u32 = 1 << 5;
/// Disabled by filter.
pub const FTRACE_FL_DISABLED: u32 = 1 << 6;
/// Direct call (direct trampoline).
pub const FTRACE_FL_DIRECT: u32 = 1 << 7;
/// Direct call enabled.
pub const FTRACE_FL_DIRECT_EN: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Tracer names
// ---------------------------------------------------------------------------

/// No-op tracer.
pub const FTRACE_TRACER_NOP: &str = "nop";
/// Function tracer.
pub const FTRACE_TRACER_FUNCTION: &str = "function";
/// Function graph tracer.
pub const FTRACE_TRACER_FUNCTION_GRAPH: &str = "function_graph";
/// IRQs-off latency tracer.
pub const FTRACE_TRACER_IRQSOFF: &str = "irqsoff";
/// Preempt-off latency tracer.
pub const FTRACE_TRACER_PREEMPTOFF: &str = "preemptoff";
/// Preempt+IRQ off tracer.
pub const FTRACE_TRACER_PREEMPTIRQSOFF: &str = "preemptirqsoff";
/// Wakeup latency tracer.
pub const FTRACE_TRACER_WAKEUP: &str = "wakeup";
/// Wakeup RT latency tracer.
pub const FTRACE_TRACER_WAKEUP_RT: &str = "wakeup_rt";
/// Wakeup DL latency tracer.
pub const FTRACE_TRACER_WAKEUP_DL: &str = "wakeup_dl";
/// Hardware latency tracer.
pub const FTRACE_TRACER_HWLAT: &str = "hwlat";
/// OS noise tracer.
pub const FTRACE_TRACER_OSNOISE: &str = "osnoise";
/// Timer latency tracer.
pub const FTRACE_TRACER_TIMERLAT: &str = "timerlat";
/// Branch tracer.
pub const FTRACE_TRACER_BRANCH: &str = "branch";
/// Block tracer.
pub const FTRACE_TRACER_BLK: &str = "blk";

// ---------------------------------------------------------------------------
// Ftrace operations flags
// ---------------------------------------------------------------------------

/// Ops: recursion safe.
pub const FTRACE_OPS_FL_RECURSION: u32 = 1 << 0;
/// Ops: stub (placeholder).
pub const FTRACE_OPS_FL_STUB: u32 = 1 << 1;
/// Ops: initialized.
pub const FTRACE_OPS_FL_INITIALIZED: u32 = 1 << 2;
/// Ops: deleted.
pub const FTRACE_OPS_FL_DELETED: u32 = 1 << 3;
/// Ops: adding.
pub const FTRACE_OPS_FL_ADDING: u32 = 1 << 4;
/// Ops: removing.
pub const FTRACE_OPS_FL_REMOVING: u32 = 1 << 5;
/// Ops: dynamic.
pub const FTRACE_OPS_FL_DYNAMIC: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fl_flags_powers_of_two() {
        let flags = [
            FTRACE_FL_ENABLED, FTRACE_FL_REGS, FTRACE_FL_REGS_EN,
            FTRACE_FL_TRAMP, FTRACE_FL_TRAMP_EN, FTRACE_FL_IPMODIFY,
            FTRACE_FL_DISABLED, FTRACE_FL_DIRECT, FTRACE_FL_DIRECT_EN,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_fl_flags_no_overlap() {
        let flags = [
            FTRACE_FL_ENABLED, FTRACE_FL_REGS, FTRACE_FL_REGS_EN,
            FTRACE_FL_TRAMP, FTRACE_FL_TRAMP_EN, FTRACE_FL_IPMODIFY,
            FTRACE_FL_DISABLED, FTRACE_FL_DIRECT, FTRACE_FL_DIRECT_EN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tracer_names_distinct() {
        let names = [
            FTRACE_TRACER_NOP, FTRACE_TRACER_FUNCTION,
            FTRACE_TRACER_FUNCTION_GRAPH, FTRACE_TRACER_IRQSOFF,
            FTRACE_TRACER_PREEMPTOFF, FTRACE_TRACER_PREEMPTIRQSOFF,
            FTRACE_TRACER_WAKEUP, FTRACE_TRACER_WAKEUP_RT,
            FTRACE_TRACER_WAKEUP_DL, FTRACE_TRACER_HWLAT,
            FTRACE_TRACER_OSNOISE, FTRACE_TRACER_TIMERLAT,
            FTRACE_TRACER_BRANCH, FTRACE_TRACER_BLK,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_ops_flags_powers_of_two() {
        let flags = [
            FTRACE_OPS_FL_RECURSION, FTRACE_OPS_FL_STUB,
            FTRACE_OPS_FL_INITIALIZED, FTRACE_OPS_FL_DELETED,
            FTRACE_OPS_FL_ADDING, FTRACE_OPS_FL_REMOVING,
            FTRACE_OPS_FL_DYNAMIC,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }
}
