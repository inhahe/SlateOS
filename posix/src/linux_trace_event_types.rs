//! `<linux/trace_events.h>` — trace event type and flag constants.
//!
//! Trace events are the kernel's structured logging mechanism. Each
//! event has a defined format with typed fields, recorded into a
//! per-CPU ring buffer. Tools like `trace-cmd`, `perf`, and BPF
//! programs consume these events. The event subsystem supports
//! enabling/disabling events, filtering, and triggering actions.

// ---------------------------------------------------------------------------
// Trace event types (trace_type)
// ---------------------------------------------------------------------------

/// Function call trace.
pub const TRACE_FN: u32 = 1;
/// Context switch trace.
pub const TRACE_CTX: u32 = 2;
/// Wake-up trace.
pub const TRACE_WAKE: u32 = 3;
/// User stack trace.
pub const TRACE_STACK: u32 = 4;
/// Kernel printk trace.
pub const TRACE_PRINT: u32 = 5;
/// Binary printk trace.
pub const TRACE_BPRINT: u32 = 6;
/// Mmiotrace read.
pub const TRACE_MMIO_RW: u32 = 7;
/// Mmiotrace map.
pub const TRACE_MMIO_MAP: u32 = 8;
/// Branch tracer.
pub const TRACE_BRANCH: u32 = 9;
/// Function graph entry.
pub const TRACE_GRAPH_ENT: u32 = 10;
/// Function graph return.
pub const TRACE_GRAPH_RET: u32 = 11;
/// User event (user_events).
pub const TRACE_USER_STACK: u32 = 12;
/// Hardware latency tracer.
pub const TRACE_HWLAT: u32 = 13;
/// Raw data trace.
pub const TRACE_RAW_DATA: u32 = 14;

// ---------------------------------------------------------------------------
// Trace event flags
// ---------------------------------------------------------------------------

/// Event is enabled.
pub const EVENT_FILE_FL_ENABLED: u32 = 1 << 0;
/// Event was recorded.
pub const EVENT_FILE_FL_RECORDED_CMD: u32 = 1 << 1;
/// Event recorded tgid.
pub const EVENT_FILE_FL_RECORDED_TGID: u32 = 1 << 2;
/// Event has trigger.
pub const EVENT_FILE_FL_TRIGGER_MODE: u32 = 1 << 3;
/// Event trigger has condition.
pub const EVENT_FILE_FL_TRIGGER_COND: u32 = 1 << 4;
/// Event has PID filter.
pub const EVENT_FILE_FL_PID_FILTER: u32 = 1 << 5;
/// Event was soft-disabled.
pub const EVENT_FILE_FL_SOFT_DISABLED: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_types_distinct() {
        let types = [
            TRACE_FN, TRACE_CTX, TRACE_WAKE, TRACE_STACK,
            TRACE_PRINT, TRACE_BPRINT, TRACE_MMIO_RW,
            TRACE_MMIO_MAP, TRACE_BRANCH, TRACE_GRAPH_ENT,
            TRACE_GRAPH_RET, TRACE_USER_STACK, TRACE_HWLAT,
            TRACE_RAW_DATA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_event_flags_no_overlap() {
        let flags = [
            EVENT_FILE_FL_ENABLED, EVENT_FILE_FL_RECORDED_CMD,
            EVENT_FILE_FL_RECORDED_TGID, EVENT_FILE_FL_TRIGGER_MODE,
            EVENT_FILE_FL_TRIGGER_COND, EVENT_FILE_FL_PID_FILTER,
            EVENT_FILE_FL_SOFT_DISABLED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_graph_trace_types() {
        assert_eq!(TRACE_GRAPH_RET, TRACE_GRAPH_ENT + 1);
    }
}
