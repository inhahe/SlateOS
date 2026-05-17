//! `<linux/tracefs.h>` — Tracefs filesystem constants.
//!
//! Tracefs (/sys/kernel/tracing, formerly /sys/kernel/debug/tracing)
//! is the filesystem interface to the kernel's tracing infrastructure.
//! It exposes control files for ftrace, kprobes, uprobes, tracepoints,
//! and event tracing. Users interact with tracing by reading/writing
//! files like trace_pipe, current_tracer, events/*/enable, and
//! trace_marker. Tracefs was split from debugfs to allow tracing
//! without full debug access.

// ---------------------------------------------------------------------------
// Tracefs file types (major interface files)
// ---------------------------------------------------------------------------

/// trace — snapshot of the ring buffer.
pub const TRACEFS_FILE_TRACE: u32 = 0;
/// trace_pipe — streaming live trace data.
pub const TRACEFS_FILE_TRACE_PIPE: u32 = 1;
/// current_tracer — select active tracer.
pub const TRACEFS_FILE_CURRENT_TRACER: u32 = 2;
/// available_tracers — list supported tracers.
pub const TRACEFS_FILE_AVAILABLE_TRACERS: u32 = 3;
/// tracing_on — global enable/disable (1/0).
pub const TRACEFS_FILE_TRACING_ON: u32 = 4;
/// trace_marker — write markers from userspace.
pub const TRACEFS_FILE_TRACE_MARKER: u32 = 5;
/// trace_marker_raw — write binary data markers.
pub const TRACEFS_FILE_TRACE_MARKER_RAW: u32 = 6;
/// trace_clock — select timestamp source.
pub const TRACEFS_FILE_TRACE_CLOCK: u32 = 7;
/// buffer_size_kb — ring buffer size.
pub const TRACEFS_FILE_BUFFER_SIZE: u32 = 8;
/// set_ftrace_filter — function filter list.
pub const TRACEFS_FILE_SET_FTRACE_FILTER: u32 = 9;
/// set_ftrace_notrace — function exclusion list.
pub const TRACEFS_FILE_SET_FTRACE_NOTRACE: u32 = 10;
/// set_graph_function — graph tracer function filter.
pub const TRACEFS_FILE_SET_GRAPH_FUNCTION: u32 = 11;

// ---------------------------------------------------------------------------
// Tracefs clock sources
// ---------------------------------------------------------------------------

/// Local clock (per-CPU, fast, not synchronized).
pub const TRACEFS_CLOCK_LOCAL: u32 = 0;
/// Global clock (synchronized across CPUs, slower).
pub const TRACEFS_CLOCK_GLOBAL: u32 = 1;
/// Monotonic clock (CLOCK_MONOTONIC).
pub const TRACEFS_CLOCK_MONOTONIC: u32 = 2;
/// Monotonic raw clock (not NTP-adjusted).
pub const TRACEFS_CLOCK_MONOTONIC_RAW: u32 = 3;
/// Boot clock (includes suspend time).
pub const TRACEFS_CLOCK_BOOT: u32 = 4;
/// Perf clock (for correlation with perf events).
pub const TRACEFS_CLOCK_PERF: u32 = 5;
/// TAI clock (International Atomic Time).
pub const TRACEFS_CLOCK_TAI: u32 = 6;

// ---------------------------------------------------------------------------
// Tracefs event enable values
// ---------------------------------------------------------------------------

/// Event disabled.
pub const TRACEFS_EVENT_DISABLED: u32 = 0;
/// Event enabled.
pub const TRACEFS_EVENT_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tracefs buffer flags
// ---------------------------------------------------------------------------

/// Overwrite mode (drop oldest on overflow).
pub const TRACEFS_BUF_OVERWRITE: u32 = 1 << 0;
/// Disable buffer on trigger.
pub const TRACEFS_BUF_DISABLE_ON_FREE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_distinct() {
        let files = [
            TRACEFS_FILE_TRACE, TRACEFS_FILE_TRACE_PIPE,
            TRACEFS_FILE_CURRENT_TRACER, TRACEFS_FILE_AVAILABLE_TRACERS,
            TRACEFS_FILE_TRACING_ON, TRACEFS_FILE_TRACE_MARKER,
            TRACEFS_FILE_TRACE_MARKER_RAW, TRACEFS_FILE_TRACE_CLOCK,
            TRACEFS_FILE_BUFFER_SIZE, TRACEFS_FILE_SET_FTRACE_FILTER,
            TRACEFS_FILE_SET_FTRACE_NOTRACE,
            TRACEFS_FILE_SET_GRAPH_FUNCTION,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_clocks_distinct() {
        let clocks = [
            TRACEFS_CLOCK_LOCAL, TRACEFS_CLOCK_GLOBAL,
            TRACEFS_CLOCK_MONOTONIC, TRACEFS_CLOCK_MONOTONIC_RAW,
            TRACEFS_CLOCK_BOOT, TRACEFS_CLOCK_PERF, TRACEFS_CLOCK_TAI,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_event_states_distinct() {
        assert_ne!(TRACEFS_EVENT_DISABLED, TRACEFS_EVENT_ENABLED);
    }

    #[test]
    fn test_buffer_flags_no_overlap() {
        let flags = [TRACEFS_BUF_OVERWRITE, TRACEFS_BUF_DISABLE_ON_FREE];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }
}
