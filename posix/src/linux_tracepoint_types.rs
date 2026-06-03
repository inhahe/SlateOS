//! `<linux/tracepoint.h>` — tracepoint infrastructure constants.
//!
//! Tracepoints are static instrumentation sites compiled into the
//! kernel. Each tracepoint is a named hook that trace backends
//! (ftrace, perf, BPF) can attach callbacks to at runtime. When
//! disabled, the overhead is near zero (a single NOP or conditional
//! branch). Tracepoints provide stable instrumentation points for
//! kernel debugging and performance analysis.

// ---------------------------------------------------------------------------
// Tracepoint states
// ---------------------------------------------------------------------------

/// Tracepoint is disabled (no callbacks attached).
pub const TRACEPOINT_STATE_DISABLED: u32 = 0;
/// Tracepoint is enabled (one or more callbacks active).
pub const TRACEPOINT_STATE_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tracepoint flags
// ---------------------------------------------------------------------------

/// Tracepoint has been registered with the subsystem.
pub const TP_FLAG_REGISTERED: u32 = 1 << 0;
/// Tracepoint is being traced by ftrace.
pub const TP_FLAG_TRACE: u32 = 1 << 1;
/// Tracepoint is being profiled by perf.
pub const TP_FLAG_PROFILE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Trace iterator control flags (trace_iterator)
// ---------------------------------------------------------------------------

/// Print function name in output.
pub const TRACE_ITER_PRINT_PARENT: u32 = 1 << 0;
/// Print latency-related data.
pub const TRACE_ITER_SYM_OFFSET: u32 = 1 << 1;
/// Print symbol addresses.
pub const TRACE_ITER_SYM_ADDR: u32 = 1 << 2;
/// Verbose output.
pub const TRACE_ITER_VERBOSE: u32 = 1 << 3;
/// Raw output (no formatting).
pub const TRACE_ITER_RAW: u32 = 1 << 4;
/// Hex output.
pub const TRACE_ITER_HEX: u32 = 1 << 5;
/// Binary output.
pub const TRACE_ITER_BIN: u32 = 1 << 6;
/// Show context info (PID, CPU, timestamp).
pub const TRACE_ITER_CONTEXT_INFO: u32 = 1 << 7;
/// Show latency information.
pub const TRACE_ITER_LATENCY_FMT: u32 = 1 << 8;
/// Show IRQ info.
pub const TRACE_ITER_IRQ_INFO: u32 = 1 << 9;
/// Show function graph output.
pub const TRACE_ITER_FUNCTION: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        assert_ne!(TRACEPOINT_STATE_DISABLED, TRACEPOINT_STATE_ENABLED);
    }

    #[test]
    fn test_tp_flags_no_overlap() {
        let flags = [TP_FLAG_REGISTERED, TP_FLAG_TRACE, TP_FLAG_PROFILE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_iter_flags_no_overlap() {
        let flags = [
            TRACE_ITER_PRINT_PARENT,
            TRACE_ITER_SYM_OFFSET,
            TRACE_ITER_SYM_ADDR,
            TRACE_ITER_VERBOSE,
            TRACE_ITER_RAW,
            TRACE_ITER_HEX,
            TRACE_ITER_BIN,
            TRACE_ITER_CONTEXT_INFO,
            TRACE_ITER_LATENCY_FMT,
            TRACE_ITER_IRQ_INFO,
            TRACE_ITER_FUNCTION,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
