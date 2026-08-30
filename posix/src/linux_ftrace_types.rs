//! `<linux/ftrace.h>` — Function tracer (ftrace) framework constants.
//!
//! ftrace is the Linux kernel's primary tracing infrastructure. It
//! can trace function calls (entry/exit), measure latencies, track
//! context switches, and generate function call graphs. ftrace uses
//! compiler instrumentation (mcount/fentry) to patch function
//! prologues at runtime. It supports multiple tracers (function,
//! function_graph, irqsoff, wakeup, etc.) and integrates with
//! perf_events, tracepoints, and kprobes.

// ---------------------------------------------------------------------------
// ftrace tracer types
// ---------------------------------------------------------------------------

/// No tracer active.
pub const FTRACE_TRACER_NOP: u32 = 0;
/// Function call tracer (logs entry to each function).
pub const FTRACE_TRACER_FUNCTION: u32 = 1;
/// Function graph tracer (logs entry + exit with timing).
pub const FTRACE_TRACER_FUNCTION_GRAPH: u32 = 2;
/// IRQs-off latency tracer.
pub const FTRACE_TRACER_IRQSOFF: u32 = 3;
/// Preemption-off latency tracer.
pub const FTRACE_TRACER_PREEMPTOFF: u32 = 4;
/// IRQs or preemption off latency tracer.
pub const FTRACE_TRACER_PREEMPTIRQSOFF: u32 = 5;
/// Wakeup latency tracer (normal tasks).
pub const FTRACE_TRACER_WAKEUP: u32 = 6;
/// Wakeup latency tracer (RT tasks).
pub const FTRACE_TRACER_WAKEUP_RT: u32 = 7;
/// Hardware latency detector.
pub const FTRACE_TRACER_HWLAT: u32 = 8;
/// OS noise tracer.
pub const FTRACE_TRACER_OSNOISE: u32 = 9;
/// Timer latency tracer.
pub const FTRACE_TRACER_TIMERLAT: u32 = 10;
/// Branch tracer (likely/unlikely profiling).
pub const FTRACE_TRACER_BRANCH: u32 = 11;

// ---------------------------------------------------------------------------
// ftrace options (trace_options bitmask)
// ---------------------------------------------------------------------------

/// Print CPU number.
pub const FTRACE_OPT_PRINT_CPU: u32 = 1 << 0;
/// Print function names (not addresses).
pub const FTRACE_OPT_FUNCNAME: u32 = 1 << 1;
/// Print timestamp.
pub const FTRACE_OPT_TIMESTAMP: u32 = 1 << 2;
/// Print process name/PID.
pub const FTRACE_OPT_PROC: u32 = 1 << 3;
/// Print latency format.
pub const FTRACE_OPT_LATENCY: u32 = 1 << 4;
/// Enable function graph overhead markers.
pub const FTRACE_OPT_GRAPH_OVERHEAD: u32 = 1 << 5;
/// Enable function graph duration.
pub const FTRACE_OPT_GRAPH_DURATION: u32 = 1 << 6;
/// Print absolute timestamps.
pub const FTRACE_OPT_GRAPH_ABS_TIME: u32 = 1 << 7;
/// Enable IRQ info in output.
pub const FTRACE_OPT_IRQ_INFO: u32 = 1 << 8;
/// Enable stacktrace on each event.
pub const FTRACE_OPT_STACKTRACE: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// ftrace function filter flags
// ---------------------------------------------------------------------------

/// Filter: trace this function.
pub const FTRACE_FL_ENABLED: u32 = 1 << 0;
/// Filter: function has been modified (patched).
pub const FTRACE_FL_MODIFIED: u32 = 1 << 1;
/// Filter: direct trampoline attached.
pub const FTRACE_FL_DIRECT: u32 = 1 << 2;
/// Filter: function is being traced via kprobe.
pub const FTRACE_FL_KPROBE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// ftrace events (trace ring buffer entry types)
// ---------------------------------------------------------------------------

/// Function entry event.
pub const FTRACE_EVENT_FUNCENTRY: u32 = 1;
/// Function return event (function_graph).
pub const FTRACE_EVENT_FUNCRETURN: u32 = 2;
/// Context switch event.
pub const FTRACE_EVENT_CTXSWITCH: u32 = 3;
/// Wakeup event.
pub const FTRACE_EVENT_WAKEUP: u32 = 4;
/// User-defined marker (trace_marker write).
pub const FTRACE_EVENT_PRINT: u32 = 5;
/// Stack trace event.
pub const FTRACE_EVENT_STACK: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracers_distinct() {
        let tracers = [
            FTRACE_TRACER_NOP,
            FTRACE_TRACER_FUNCTION,
            FTRACE_TRACER_FUNCTION_GRAPH,
            FTRACE_TRACER_IRQSOFF,
            FTRACE_TRACER_PREEMPTOFF,
            FTRACE_TRACER_PREEMPTIRQSOFF,
            FTRACE_TRACER_WAKEUP,
            FTRACE_TRACER_WAKEUP_RT,
            FTRACE_TRACER_HWLAT,
            FTRACE_TRACER_OSNOISE,
            FTRACE_TRACER_TIMERLAT,
            FTRACE_TRACER_BRANCH,
        ];
        for i in 0..tracers.len() {
            for j in (i + 1)..tracers.len() {
                assert_ne!(tracers[i], tracers[j]);
            }
        }
    }

    #[test]
    fn test_options_no_overlap() {
        let opts = [
            FTRACE_OPT_PRINT_CPU,
            FTRACE_OPT_FUNCNAME,
            FTRACE_OPT_TIMESTAMP,
            FTRACE_OPT_PROC,
            FTRACE_OPT_LATENCY,
            FTRACE_OPT_GRAPH_OVERHEAD,
            FTRACE_OPT_GRAPH_DURATION,
            FTRACE_OPT_GRAPH_ABS_TIME,
            FTRACE_OPT_IRQ_INFO,
            FTRACE_OPT_STACKTRACE,
        ];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            FTRACE_FL_ENABLED,
            FTRACE_FL_MODIFIED,
            FTRACE_FL_DIRECT,
            FTRACE_FL_KPROBE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            FTRACE_EVENT_FUNCENTRY,
            FTRACE_EVENT_FUNCRETURN,
            FTRACE_EVENT_CTXSWITCH,
            FTRACE_EVENT_WAKEUP,
            FTRACE_EVENT_PRINT,
            FTRACE_EVENT_STACK,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
