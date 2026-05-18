//! Kernel tracing (ftrace) marker and event constants.
//!
//! Writing to `/sys/kernel/debug/tracing/trace_marker` allows
//! userspace to inject markers into the kernel trace.  These
//! constants define the marker format, trace event types, and
//! tracing control paths.

// ---------------------------------------------------------------------------
// Trace marker limits
// ---------------------------------------------------------------------------

/// Maximum length of a trace_marker write (bytes).
pub const TRACE_MARKER_MAX: u32 = 65536;
/// Maximum length of a trace_marker_raw write (bytes).
pub const TRACE_MARKER_RAW_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Trace event types (ftrace internal)
// ---------------------------------------------------------------------------

/// Function call trace event.
pub const TRACE_FN: u32 = 1;
/// Context switch trace event.
pub const TRACE_CTX: u32 = 2;
/// Wakeup trace event.
pub const TRACE_WAKE: u32 = 3;
/// Stack trace event.
pub const TRACE_STACK: u32 = 4;
/// User-provided trace marker.
pub const TRACE_PRINT: u32 = 5;
/// Binary format trace marker.
pub const TRACE_BPRINT: u32 = 6;
/// Binary format user marker.
pub const TRACE_BPUTS: u32 = 7;
/// Hardware latency event.
pub const TRACE_HWLAT: u32 = 8;
/// Raw data trace event.
pub const TRACE_RAW_DATA: u32 = 9;

// ---------------------------------------------------------------------------
// Tracing control flag bits
// ---------------------------------------------------------------------------

/// Tracing is enabled.
pub const TRACE_FLAG_ENABLED: u32 = 1 << 0;
/// IRQs are off during this event.
pub const TRACE_FLAG_IRQS_OFF: u32 = 1 << 2;
/// Need reschedule flag is set.
pub const TRACE_FLAG_NEED_RESCHED: u32 = 1 << 3;
/// Hardirq context.
pub const TRACE_FLAG_HARDIRQ: u32 = 1 << 4;
/// Softirq context.
pub const TRACE_FLAG_SOFTIRQ: u32 = 1 << 5;
/// Preemption count.
pub const TRACE_FLAG_PREEMPT_RESCHED: u32 = 1 << 6;
/// NMI context.
pub const TRACE_FLAG_NMI: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_max() {
        assert_eq!(TRACE_MARKER_MAX, 65536);
    }

    #[test]
    fn test_event_types_distinct() {
        let types = [
            TRACE_FN, TRACE_CTX, TRACE_WAKE, TRACE_STACK,
            TRACE_PRINT, TRACE_BPRINT, TRACE_BPUTS,
            TRACE_HWLAT, TRACE_RAW_DATA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fn_is_one() {
        assert_eq!(TRACE_FN, 1);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TRACE_FLAG_ENABLED, TRACE_FLAG_IRQS_OFF,
            TRACE_FLAG_NEED_RESCHED, TRACE_FLAG_HARDIRQ,
            TRACE_FLAG_SOFTIRQ, TRACE_FLAG_PREEMPT_RESCHED,
            TRACE_FLAG_NMI,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            TRACE_FLAG_ENABLED, TRACE_FLAG_IRQS_OFF,
            TRACE_FLAG_NEED_RESCHED, TRACE_FLAG_HARDIRQ,
            TRACE_FLAG_SOFTIRQ, TRACE_FLAG_PREEMPT_RESCHED,
            TRACE_FLAG_NMI,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }
}
