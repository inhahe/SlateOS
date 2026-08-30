//! `<linux/trace_events.h>` — Kernel tracing event constants.
//!
//! The tracing subsystem (tracefs, typically at /sys/kernel/tracing)
//! provides event-based tracing for debugging and performance
//! analysis. Tracepoints, kprobes, uprobes, and function tracing
//! emit events into per-CPU ring buffers.

// ---------------------------------------------------------------------------
// Trace event types
// ---------------------------------------------------------------------------

/// Function entry.
pub const TRACE_FN: u32 = 1;
/// Context switch.
pub const TRACE_CTX: u32 = 2;
/// Wake event.
pub const TRACE_WAKE: u32 = 3;
/// Stack trace.
pub const TRACE_STACK: u32 = 4;
/// Print event.
pub const TRACE_PRINT: u32 = 5;
/// Binary print.
pub const TRACE_BPRINT: u32 = 6;
/// Branch trace.
pub const TRACE_BRANCH: u32 = 7;
/// Function graph entry.
pub const TRACE_GRAPH_ENT: u32 = 8;
/// Function graph return.
pub const TRACE_GRAPH_RET: u32 = 9;
/// User stack.
pub const TRACE_USER_STACK: u32 = 10;
/// Hardware latency.
pub const TRACE_HWLAT: u32 = 11;
/// IRQ event.
pub const TRACE_IRQ: u32 = 12;
/// Osnoise event.
pub const TRACE_OSNOISE: u32 = 13;
/// Timerlat event.
pub const TRACE_TIMERLAT: u32 = 14;

// ---------------------------------------------------------------------------
// Trace flags (for trace_options)
// ---------------------------------------------------------------------------

/// Print IRQ info.
pub const TRACE_ITER_PRINT_PARENT: u32 = 1 << 0;
/// Sym-offset.
pub const TRACE_ITER_SYM_OFFSET: u32 = 1 << 1;
/// Sym-addr.
pub const TRACE_ITER_SYM_ADDR: u32 = 1 << 2;
/// Verbose output.
pub const TRACE_ITER_VERBOSE: u32 = 1 << 3;
/// Raw output.
pub const TRACE_ITER_RAW: u32 = 1 << 4;
/// Hex output.
pub const TRACE_ITER_HEX: u32 = 1 << 5;
/// Binary output.
pub const TRACE_ITER_BIN: u32 = 1 << 6;
/// Block output.
pub const TRACE_ITER_BLOCK: u32 = 1 << 7;
/// Stacktrace on events.
pub const TRACE_ITER_STACKTRACE: u32 = 1 << 8;
/// Print PID on each line.
pub const TRACE_ITER_PRINTK: u32 = 1 << 9;
/// IRQ info on each event.
pub const TRACE_ITER_IRQ_INFO: u32 = 1 << 10;
/// Show function call graph.
pub const TRACE_ITER_GRAPH_TIME: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// Ring buffer constants
// ---------------------------------------------------------------------------

/// Default per-CPU buffer size (KB).
pub const TRACE_BUF_SIZE_DEFAULT: u32 = 1408;
/// Minimum per-CPU buffer size (KB).
pub const TRACE_BUF_SIZE_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            TRACE_FN,
            TRACE_CTX,
            TRACE_WAKE,
            TRACE_STACK,
            TRACE_PRINT,
            TRACE_BPRINT,
            TRACE_BRANCH,
            TRACE_GRAPH_ENT,
            TRACE_GRAPH_RET,
            TRACE_USER_STACK,
            TRACE_HWLAT,
            TRACE_IRQ,
            TRACE_OSNOISE,
            TRACE_TIMERLAT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_iter_flags_powers_of_two() {
        let flags = [
            TRACE_ITER_PRINT_PARENT,
            TRACE_ITER_SYM_OFFSET,
            TRACE_ITER_SYM_ADDR,
            TRACE_ITER_VERBOSE,
            TRACE_ITER_RAW,
            TRACE_ITER_HEX,
            TRACE_ITER_BIN,
            TRACE_ITER_BLOCK,
            TRACE_ITER_STACKTRACE,
            TRACE_ITER_PRINTK,
            TRACE_ITER_IRQ_INFO,
            TRACE_ITER_GRAPH_TIME,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
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
            TRACE_ITER_BLOCK,
            TRACE_ITER_STACKTRACE,
            TRACE_ITER_PRINTK,
            TRACE_ITER_IRQ_INFO,
            TRACE_ITER_GRAPH_TIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_buf_sizes() {
        assert!(TRACE_BUF_SIZE_MIN < TRACE_BUF_SIZE_DEFAULT);
    }
}
