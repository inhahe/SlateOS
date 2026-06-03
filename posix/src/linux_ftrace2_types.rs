//! `<linux/ftrace.h>` — Additional ftrace constants.
//!
//! Supplementary ftrace constants covering tracer types,
//! function graph flags, and ftrace options.

// ---------------------------------------------------------------------------
// Tracer types
// ---------------------------------------------------------------------------

/// NOP tracer.
pub const TRACER_NOP: u32 = 0;
/// Function tracer.
pub const TRACER_FUNCTION: u32 = 1;
/// Function graph tracer.
pub const TRACER_FUNCTION_GRAPH: u32 = 2;
/// Irqsoff tracer.
pub const TRACER_IRQSOFF: u32 = 3;
/// Preemptoff tracer.
pub const TRACER_PREEMPTOFF: u32 = 4;
/// Preemptirqsoff tracer.
pub const TRACER_PREEMPTIRQSOFF: u32 = 5;
/// Wakeup tracer.
pub const TRACER_WAKEUP: u32 = 6;
/// Wakeup RT tracer.
pub const TRACER_WAKEUP_RT: u32 = 7;
/// Wakeup DL tracer.
pub const TRACER_WAKEUP_DL: u32 = 8;
/// MMIO tracer.
pub const TRACER_MMIOTRACE: u32 = 9;
/// BLK tracer.
pub const TRACER_BLK: u32 = 10;
/// Hardware latency tracer.
pub const TRACER_HWLAT: u32 = 11;
/// OS noise tracer.
pub const TRACER_OSNOISE: u32 = 12;
/// Timer latency tracer.
pub const TRACER_TIMERLAT: u32 = 13;

// ---------------------------------------------------------------------------
// Function graph flags
// ---------------------------------------------------------------------------

/// Tail call.
pub const FTRACE_GRAPH_TAIL_CALL: u32 = 1 << 0;
/// Sleep entry.
pub const FTRACE_GRAPH_SLEEP: u32 = 1 << 1;
/// Graph notrace.
pub const FTRACE_GRAPH_NOTRACE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Ftrace options (trace_options)
// ---------------------------------------------------------------------------

/// Print parent function.
pub const TRACE_OPT_PRINT_PARENT: u32 = 1 << 0;
/// Show symbols.
pub const TRACE_OPT_SYM_OFFSET: u32 = 1 << 1;
/// Show symbol address.
pub const TRACE_OPT_SYM_ADDR: u32 = 1 << 2;
/// Verbose output.
pub const TRACE_OPT_VERBOSE: u32 = 1 << 3;
/// Raw format.
pub const TRACE_OPT_RAW: u32 = 1 << 4;
/// Hex format.
pub const TRACE_OPT_HEX: u32 = 1 << 5;
/// Binary format.
pub const TRACE_OPT_BIN: u32 = 1 << 6;
/// Show block markers.
pub const TRACE_OPT_BLOCK: u32 = 1 << 7;
/// Stack trace on printk.
pub const TRACE_OPT_STACKTRACE: u32 = 1 << 8;
/// Show trace_printk.
pub const TRACE_OPT_TRACE_PRINTK: u32 = 1 << 9;
/// Annotate graph.
pub const TRACE_OPT_ANNOTATE: u32 = 1 << 10;
/// Function fork record.
pub const TRACE_OPT_RECORD_CMD: u32 = 1 << 11;
/// Overwrite (ring buffer).
pub const TRACE_OPT_OVERWRITE: u32 = 1 << 12;
/// Disable on free.
pub const TRACE_OPT_DISABLE_ON_FREE: u32 = 1 << 13;
/// IRQ info.
pub const TRACE_OPT_IRQ_INFO: u32 = 1 << 14;
/// Markers.
pub const TRACE_OPT_MARKERS: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracer_types_distinct() {
        let types = [
            TRACER_NOP,
            TRACER_FUNCTION,
            TRACER_FUNCTION_GRAPH,
            TRACER_IRQSOFF,
            TRACER_PREEMPTOFF,
            TRACER_PREEMPTIRQSOFF,
            TRACER_WAKEUP,
            TRACER_WAKEUP_RT,
            TRACER_WAKEUP_DL,
            TRACER_MMIOTRACE,
            TRACER_BLK,
            TRACER_HWLAT,
            TRACER_OSNOISE,
            TRACER_TIMERLAT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_graph_flags_power_of_two() {
        let flags = [
            FTRACE_GRAPH_TAIL_CALL,
            FTRACE_GRAPH_SLEEP,
            FTRACE_GRAPH_NOTRACE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_graph_flags_no_overlap() {
        let flags = [
            FTRACE_GRAPH_TAIL_CALL,
            FTRACE_GRAPH_SLEEP,
            FTRACE_GRAPH_NOTRACE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_options_power_of_two() {
        let opts = [
            TRACE_OPT_PRINT_PARENT,
            TRACE_OPT_SYM_OFFSET,
            TRACE_OPT_SYM_ADDR,
            TRACE_OPT_VERBOSE,
            TRACE_OPT_RAW,
            TRACE_OPT_HEX,
            TRACE_OPT_BIN,
            TRACE_OPT_BLOCK,
            TRACE_OPT_STACKTRACE,
            TRACE_OPT_TRACE_PRINTK,
            TRACE_OPT_ANNOTATE,
            TRACE_OPT_RECORD_CMD,
            TRACE_OPT_OVERWRITE,
            TRACE_OPT_DISABLE_ON_FREE,
            TRACE_OPT_IRQ_INFO,
            TRACE_OPT_MARKERS,
        ];
        for o in &opts {
            assert!(o.is_power_of_two(), "0x{:08x} not power of two", o);
        }
    }

    #[test]
    fn test_options_no_overlap() {
        let opts = [
            TRACE_OPT_PRINT_PARENT,
            TRACE_OPT_SYM_OFFSET,
            TRACE_OPT_SYM_ADDR,
            TRACE_OPT_VERBOSE,
            TRACE_OPT_RAW,
            TRACE_OPT_HEX,
            TRACE_OPT_BIN,
            TRACE_OPT_BLOCK,
            TRACE_OPT_STACKTRACE,
            TRACE_OPT_TRACE_PRINTK,
            TRACE_OPT_ANNOTATE,
            TRACE_OPT_RECORD_CMD,
            TRACE_OPT_OVERWRITE,
            TRACE_OPT_DISABLE_ON_FREE,
            TRACE_OPT_IRQ_INFO,
            TRACE_OPT_MARKERS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }
}
